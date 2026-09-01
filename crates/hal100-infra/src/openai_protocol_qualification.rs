use serde::Deserialize;
use serde_json::{Value, json};
use std::time::Instant;

use crate::{
    BoundedEngineHttpClient, EngineHttpError, ExternalEngineAdapterError, VerifiedEngineTarget,
};

const MAX_MODEL_ID_BYTES: usize = 256;
const MAX_QUALIFICATION_BODY_BYTES: usize = 4 * 1024 * 1024;
const MAX_QUALIFICATION_EVENTS: usize = 4096;
const MAX_FINGERPRINT_BYTES: usize = 512;
const STABILITY_REQUESTS: usize = 20;
const STABILITY_CONCURRENCY: usize = 4;
const MAX_STABILITY_LATENCY_MS: u128 = 120_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenAiQualificationObservation {
    pub system_fingerprint: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct OpenAiQualificationOptions {
    pub chat_template_kwargs: Option<Value>,
    pub reasoning_effort: Option<OpenAiQualificationReasoningEffort>,
    /// Some engines expose a documented OpenAI compatibility quirk that HAL100 normalizes at the
    /// Gateway boundary. Keep strict JSON-string arguments as the default; adapters may opt into
    /// structured arguments only when the same engine-bound compatibility transform is tested.
    pub tool_arguments: OpenAiToolArgumentsMode,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum OpenAiToolArgumentsMode {
    #[default]
    JsonString,
    JsonStringOrObject,
}

/// Reasoning controls that HAL100 has explicitly qualified for protocol probes.
///
/// This deliberately exposes only the disabled state. Qualification must measure the Agent
/// protocol itself rather than spend a small, fixed output budget on a model's private reasoning
/// trace. Additional effort levels can be added only when a concrete engine acceptance contract
/// needs and verifies them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenAiQualificationReasoningEffort {
    Disabled,
}

impl OpenAiQualificationReasoningEffort {
    fn as_openai_value(self) -> &'static str {
        match self {
            Self::Disabled => "none",
        }
    }
}

/// Bounded repeated-request observation used by explicit real-service acceptance tests.
///
/// The observation contains only aggregate counters and latency; it never retains prompts,
/// responses, credentials or endpoint details. A passing observation is evidence that the
/// selected model handled the fixed request shape across several concurrent waves, not a claim
/// about an engine's absolute throughput outside this bounded probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenAiStabilityObservation {
    pub workload_revision: &'static str,
    pub attempts: u16,
    pub concurrency: u8,
    pub p95_latency_ms: u64,
    pub max_latency_ms: u64,
    pub total_prompt_tokens: u64,
    pub total_completion_tokens: u64,
    pub wall_time_ms: u64,
}

pub const OPENAI_STABILITY_WORKLOAD_REVISION: &str = "openai-short-chat-v1";

/// Executes the bounded Agent-critical subset shared by OpenAI-compatible engines.
///
/// A model is qualified only if one unary tool call and one streaming response both satisfy the
/// contract. Engine adapters remain responsible for translating any upstream fingerprint into a
/// trustworthy engine identity.
pub async fn qualify_openai_agent_protocol(
    http: &BoundedEngineHttpClient,
    target: &VerifiedEngineTarget,
    model_id: &str,
    options: &OpenAiQualificationOptions,
) -> Result<OpenAiQualificationObservation, ExternalEngineAdapterError> {
    validate_text(model_id, MAX_MODEL_ID_BYTES)?;
    let mut unary = json!({
        "model": model_id,
        "messages": [{
            "role": "user",
            "content": "Call hal100_protocol_probe exactly once with a short value."
        }],
        "tools": [{
            "type": "function",
            "function": {
                "name": "hal100_protocol_probe",
                "description": "HAL100 bounded protocol qualification probe",
                "parameters": {
                    "type": "object",
                    "properties": {"value": {"type": "string"}},
                    "required": ["value"],
                    "additionalProperties": false
                }
            }
        }],
        "tool_choice": {
            "type": "function",
            "function": {"name": "hal100_protocol_probe"}
        },
        "parallel_tool_calls": false,
        "temperature": 0,
        "max_tokens": 64,
        "stream": false
    });
    apply_qualification_options(&mut unary, options);
    let unary_body = http
        .post_json_bounded(
            target,
            "/v1/chat/completions",
            &unary,
            MAX_QUALIFICATION_BODY_BYTES,
        )
        .await
        .map_err(map_http_error)?;
    let unary_fingerprint = validate_unary_qualification(&unary_body, options.tool_arguments)?;

    let mut streaming = json!({
        "model": model_id,
        "messages": [{"role": "user", "content": "Reply briefly with OK."}],
        "temperature": 0,
        "max_tokens": 8,
        "stream": true,
        "stream_options": {"include_usage": true}
    });
    apply_qualification_options(&mut streaming, options);
    let stream_body = http
        .post_json_bounded(
            target,
            "/v1/chat/completions",
            &streaming,
            MAX_QUALIFICATION_BODY_BYTES,
        )
        .await
        .map_err(map_http_error)?;
    let stream_fingerprint = validate_stream_qualification(&stream_body)?;
    if let (Some(unary), Some(stream)) = (&unary_fingerprint, &stream_fingerprint)
        && unary != stream
    {
        return Err(ExternalEngineAdapterError::QualificationFailed);
    }
    Ok(OpenAiQualificationObservation {
        system_fingerprint: unary_fingerprint.or(stream_fingerprint),
    })
}

/// Execute a bounded repeated/concurrent chat probe against one already verified target.
///
/// This is intentionally separate from the protocol qualification probe: the latter proves the
/// minimum Agent contract, while this function records a small deterministic stability sample for
/// an explicit acceptance run. It does not start, restart or mutate the external service.
pub async fn qualify_openai_runtime_stability(
    http: &BoundedEngineHttpClient,
    target: &VerifiedEngineTarget,
    model_id: &str,
    options: &OpenAiQualificationOptions,
) -> Result<OpenAiStabilityObservation, ExternalEngineAdapterError> {
    validate_text(model_id, MAX_MODEL_ID_BYTES)?;
    let probe_started = Instant::now();
    let mut completed = 0usize;
    let mut latencies_ms = Vec::with_capacity(STABILITY_REQUESTS);
    let mut total_prompt_tokens = 0u64;
    let mut total_completion_tokens = 0u64;
    while completed < STABILITY_REQUESTS {
        let wave = (STABILITY_REQUESTS - completed).min(STABILITY_CONCURRENCY);
        let mut requests = Vec::with_capacity(wave);
        for _ in 0..wave {
            let mut body = json!({
                "model": model_id,
                "messages": [{"role": "user", "content": "Reply briefly with OK."}],
                "temperature": 0,
                "max_tokens": 8,
                "stream": false
            });
            apply_qualification_options(&mut body, options);
            requests.push(async move {
                let started = Instant::now();
                let response = http
                    .post_json_bounded(
                        target,
                        "/v1/chat/completions",
                        &body,
                        MAX_QUALIFICATION_BODY_BYTES,
                    )
                    .await
                    .map_err(map_http_error)?;
                let (prompt_tokens, completion_tokens) = validate_stability_response(&response)?;
                Ok::<(u128, u64, u64), ExternalEngineAdapterError>((
                    started.elapsed().as_millis(),
                    prompt_tokens,
                    completion_tokens,
                ))
            });
        }
        for result in futures_util::future::join_all(requests).await {
            let (latency, prompt_tokens, completion_tokens) = result?;
            if latency > MAX_STABILITY_LATENCY_MS {
                return Err(ExternalEngineAdapterError::QualificationFailed);
            }
            latencies_ms.push(latency);
            total_prompt_tokens = total_prompt_tokens
                .checked_add(prompt_tokens)
                .ok_or(ExternalEngineAdapterError::QualificationFailed)?;
            total_completion_tokens = total_completion_tokens
                .checked_add(completion_tokens)
                .ok_or(ExternalEngineAdapterError::QualificationFailed)?;
        }
        completed += wave;
    }
    latencies_ms.sort_unstable();
    let p95_index = latencies_ms
        .len()
        .saturating_mul(95)
        .div_ceil(100)
        .saturating_sub(1);
    let p95_latency_ms = *latencies_ms
        .get(p95_index)
        .ok_or(ExternalEngineAdapterError::QualificationFailed)?;
    let max_latency_ms = *latencies_ms
        .last()
        .ok_or(ExternalEngineAdapterError::QualificationFailed)?;
    Ok(OpenAiStabilityObservation {
        workload_revision: OPENAI_STABILITY_WORKLOAD_REVISION,
        attempts: u16::try_from(STABILITY_REQUESTS)
            .map_err(|_| ExternalEngineAdapterError::QualificationFailed)?,
        concurrency: u8::try_from(STABILITY_CONCURRENCY)
            .map_err(|_| ExternalEngineAdapterError::QualificationFailed)?,
        p95_latency_ms: u64::try_from(p95_latency_ms)
            .map_err(|_| ExternalEngineAdapterError::QualificationFailed)?,
        max_latency_ms: u64::try_from(max_latency_ms)
            .map_err(|_| ExternalEngineAdapterError::QualificationFailed)?,
        total_prompt_tokens,
        total_completion_tokens,
        wall_time_ms: u64::try_from(probe_started.elapsed().as_millis())
            .map_err(|_| ExternalEngineAdapterError::QualificationFailed)?,
    })
}

fn apply_qualification_options(body: &mut Value, options: &OpenAiQualificationOptions) {
    if let Some(chat_template_kwargs) = &options.chat_template_kwargs {
        body["chat_template_kwargs"] = chat_template_kwargs.clone();
    }
    if let Some(reasoning_effort) = options.reasoning_effort {
        body["reasoning_effort"] = Value::String(reasoning_effort.as_openai_value().to_owned());
    }
}

#[derive(Deserialize)]
struct QualificationUnaryResponse {
    choices: Vec<QualificationUnaryChoice>,
    usage: QualificationUsage,
    #[serde(default)]
    system_fingerprint: Option<String>,
}

#[derive(Deserialize)]
struct QualificationUnaryChoice {
    message: QualificationMessage,
}

#[derive(Deserialize)]
struct QualificationMessage {
    tool_calls: Vec<QualificationToolCall>,
}

#[derive(Deserialize)]
struct QualificationToolCall {
    function: QualificationFunction,
}

#[derive(Deserialize)]
struct QualificationFunction {
    name: String,
    arguments: Value,
}

#[derive(Deserialize)]
struct QualificationUsage {
    prompt_tokens: u64,
    completion_tokens: u64,
}

fn validate_unary_qualification(
    body: &[u8],
    tool_arguments: OpenAiToolArgumentsMode,
) -> Result<Option<String>, ExternalEngineAdapterError> {
    let response = serde_json::from_slice::<QualificationUnaryResponse>(body)
        .map_err(|_| ExternalEngineAdapterError::QualificationFailed)?;
    if response.choices.is_empty()
        || response.choices.len() > 8
        || response.usage.prompt_tokens == 0
        || response.usage.completion_tokens == 0
    {
        return Err(ExternalEngineAdapterError::QualificationFailed);
    }
    let calls = &response.choices[0].message.tool_calls;
    if calls.len() != 1 || calls[0].function.name != "hal100_protocol_probe" {
        return Err(ExternalEngineAdapterError::QualificationFailed);
    }
    let arguments = match &calls[0].function.arguments {
        Value::String(arguments) => {
            validate_text(arguments, 4096)
                .map_err(|_| ExternalEngineAdapterError::QualificationFailed)?;
            serde_json::from_str::<Value>(arguments)
                .map_err(|_| ExternalEngineAdapterError::QualificationFailed)?
        }
        Value::Object(arguments)
            if tool_arguments == OpenAiToolArgumentsMode::JsonStringOrObject =>
        {
            Value::Object(arguments.clone())
        }
        _ => return Err(ExternalEngineAdapterError::QualificationFailed),
    };
    if !arguments.is_object() {
        return Err(ExternalEngineAdapterError::QualificationFailed);
    }
    validate_fingerprint(response.system_fingerprint)
}

fn validate_stream_qualification(
    body: &[u8],
) -> Result<Option<String>, ExternalEngineAdapterError> {
    let text =
        std::str::from_utf8(body).map_err(|_| ExternalEngineAdapterError::QualificationFailed)?;
    let mut events = 0usize;
    let mut saw_choice = false;
    let mut saw_finish = false;
    let mut saw_usage = false;
    let mut saw_done = false;
    let mut fingerprint = None;
    for line in text.lines() {
        let Some(data) = line.trim_end_matches('\r').strip_prefix("data: ") else {
            continue;
        };
        events = events.saturating_add(1);
        if events > MAX_QUALIFICATION_EVENTS {
            return Err(ExternalEngineAdapterError::QualificationFailed);
        }
        if data == "[DONE]" {
            saw_done = true;
            continue;
        }
        let value = serde_json::from_str::<Value>(data)
            .map_err(|_| ExternalEngineAdapterError::QualificationFailed)?;
        if let Some(observed) = value.get("system_fingerprint").and_then(Value::as_str) {
            // MLC LLM's official REST server currently serializes this optional field as an
            // empty string. Treat that as unavailable identity rather than as malformed input;
            // adapters must still decide whether an absent fingerprint is sufficient for their
            // own engine-version contract.
            if !observed.is_empty() {
                validate_text(observed, MAX_FINGERPRINT_BYTES)
                    .map_err(|_| ExternalEngineAdapterError::QualificationFailed)?;
                if fingerprint
                    .as_deref()
                    .is_some_and(|existing| existing != observed)
                {
                    return Err(ExternalEngineAdapterError::QualificationFailed);
                }
                fingerprint = Some(observed.to_owned());
            }
        }
        if value
            .get("choices")
            .and_then(Value::as_array)
            .is_some_and(|choices| !choices.is_empty())
        {
            saw_choice = true;
        }
        if value
            .get("choices")
            .and_then(Value::as_array)
            .is_some_and(|choices| {
                choices.iter().any(|choice| {
                    choice
                        .get("finish_reason")
                        .is_some_and(|reason| !reason.is_null())
                })
            })
        {
            saw_finish = true;
        }
        if let Some(usage) = value.get("usage").filter(|usage| !usage.is_null()) {
            saw_usage = usage
                .get("prompt_tokens")
                .and_then(Value::as_u64)
                .is_some_and(|tokens| tokens > 0)
                && usage
                    .get("completion_tokens")
                    .and_then(Value::as_u64)
                    .is_some();
        }
    }
    if !saw_choice || !saw_finish || !saw_usage || !saw_done {
        return Err(ExternalEngineAdapterError::QualificationFailed);
    }
    Ok(fingerprint)
}

#[derive(Deserialize)]
struct StabilityResponse {
    choices: Vec<Value>,
    usage: QualificationUsage,
}

fn validate_stability_response(body: &[u8]) -> Result<(u64, u64), ExternalEngineAdapterError> {
    let response = serde_json::from_slice::<StabilityResponse>(body)
        .map_err(|_| ExternalEngineAdapterError::QualificationFailed)?;
    if response.choices.is_empty()
        || response.choices.len() > 8
        || response.usage.prompt_tokens == 0
        || response.usage.completion_tokens == 0
    {
        return Err(ExternalEngineAdapterError::QualificationFailed);
    }
    Ok((
        response.usage.prompt_tokens,
        response.usage.completion_tokens,
    ))
}

fn validate_fingerprint(
    fingerprint: Option<String>,
) -> Result<Option<String>, ExternalEngineAdapterError> {
    match fingerprint {
        Some(fingerprint) if fingerprint.is_empty() => Ok(None),
        Some(fingerprint) => {
            validate_text(&fingerprint, MAX_FINGERPRINT_BYTES)
                .map_err(|_| ExternalEngineAdapterError::QualificationFailed)?;
            Ok(Some(fingerprint))
        }
        None => Ok(None),
    }
}

pub(crate) fn validate_text(
    value: &str,
    max_bytes: usize,
) -> Result<(), ExternalEngineAdapterError> {
    if value.trim().is_empty() || value.len() > max_bytes || value.chars().any(char::is_control) {
        return Err(ExternalEngineAdapterError::InvalidResponse);
    }
    Ok(())
}

pub(crate) fn map_http_error(error: EngineHttpError) -> ExternalEngineAdapterError {
    match error {
        EngineHttpError::Client => ExternalEngineAdapterError::Client,
        EngineHttpError::Target => ExternalEngineAdapterError::InvalidEndpoint,
        EngineHttpError::Unreachable => ExternalEngineAdapterError::Unreachable,
        EngineHttpError::InvalidResponse => ExternalEngineAdapterError::InvalidResponse,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use axum::{Router, routing::post};
    use hal100_protocol::{
        EngineAdapterId, InferenceAccelerator, InferenceArchitecture, InferenceDeployment,
        InferenceEngineDescriptor, InferenceEngineKind, InferenceEngineManifest,
        InferenceEngineOwnership, InferenceEngineSupportStatus, InferenceEngineSupportUnit,
        InferenceModelFormat, InferencePlatform, InferenceProtocol,
    };
    use serde_json::json;

    use super::*;

    #[test]
    fn stream_qualification_rejects_inconsistent_engine_fingerprints() {
        let body = concat!(
            "data: {\"system_fingerprint\":\"engine-a\",\"choices\":[{\"finish_reason\":null}]}\n\n",
            "data: {\"system_fingerprint\":\"engine-b\",\"choices\":[{\"finish_reason\":\"stop\"}]}\n\n",
            "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":1}}\n\n",
            "data: [DONE]\n\n"
        );
        assert!(validate_stream_qualification(body.as_bytes()).is_err());
    }

    #[test]
    fn structured_tool_arguments_require_an_explicit_compatibility_contract() {
        let body = br#"{"choices":[{"message":{"tool_calls":[{"function":{"name":"hal100_protocol_probe","arguments":{"value":"ok"}}}]}}],"usage":{"prompt_tokens":2,"completion_tokens":1}}"#;
        assert!(validate_unary_qualification(body, OpenAiToolArgumentsMode::JsonString).is_err());
        validate_unary_qualification(body, OpenAiToolArgumentsMode::JsonStringOrObject)
            .expect("engine-bound structured arguments");
    }

    #[test]
    fn stability_response_requires_bounded_choices_and_usage() {
        let valid = br#"{"choices":[{"message":{"content":"OK"}}],"usage":{"prompt_tokens":2,"completion_tokens":1}}"#;
        assert_eq!(
            validate_stability_response(valid).expect("valid stability response"),
            (2, 1)
        );

        let invalid: [&[u8]; 3] = [
            br#"{"choices":[],"usage":{"prompt_tokens":2,"completion_tokens":1}}"#,
            br#"{"choices":[{}],"usage":{"prompt_tokens":0,"completion_tokens":1}}"#,
            br#"{"choices":[{}],"usage":{"prompt_tokens":2,"completion_tokens":0}}"#,
        ];
        for invalid in invalid {
            assert!(validate_stability_response(invalid).is_err());
        }
    }

    #[tokio::test]
    async fn bounded_stability_probe_runs_fixed_concurrent_waves() {
        let active = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let app = Router::new().route(
            "/v1/chat/completions",
            post({
                let active = active.clone();
                let peak = peak.clone();
                move |axum::Json(body): axum::Json<Value>| async move {
                    assert_eq!(
                        body.get("reasoning_effort").and_then(Value::as_str),
                        Some("none")
                    );
                    let current = active.fetch_add(1, Ordering::AcqRel) + 1;
                    peak.fetch_max(current, Ordering::AcqRel);
                    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                    active.fetch_sub(1, Ordering::AcqRel);
                    axum::Json(json!({
                        "choices": [{"message": {"content": "OK"}}],
                        "usage": {"prompt_tokens": 2, "completion_tokens": 1}
                    }))
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener");
        let port = listener.local_addr().expect("address").port();
        let server = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("stability test server");
        });
        let manifest = InferenceEngineManifest {
            adapter_id: EngineAdapterId {
                engine: InferenceEngineKind::Vllm,
                variant: "stability-test".to_owned(),
                contract_revision: hal100_protocol::ENGINE_ADAPTER_CONTRACT_REVISION.to_owned(),
            },
            descriptor: InferenceEngineDescriptor {
                kind: InferenceEngineKind::Vllm,
                display_name: "stability test".to_owned(),
                ownership: InferenceEngineOwnership::External,
                deployment: InferenceDeployment::Local,
                protocols: vec![InferenceProtocol::OpenAi],
                platforms: vec![InferencePlatform::MacOs],
                architectures: vec![InferenceArchitecture::Aarch64],
                accelerators: vec![InferenceAccelerator::Cpu],
                model_formats: vec![InferenceModelFormat::Safetensors],
                managed_lifecycle: false,
            },
            support_units: vec![InferenceEngineSupportUnit {
                platform: InferencePlatform::MacOs,
                architecture: InferenceArchitecture::Aarch64,
                accelerator: InferenceAccelerator::Cpu,
                deployment: InferenceDeployment::Local,
                status: InferenceEngineSupportStatus::Connected,
                evidence: None,
            }],
        };
        let target = VerifiedEngineTarget::external_local(
            "stability-test",
            &manifest,
            &format!("http://127.0.0.1:{port}/v1/"),
            1,
        )
        .expect("target");
        let http = BoundedEngineHttpClient::new("stability-test").expect("client");
        let observation = qualify_openai_runtime_stability(
            &http,
            &target,
            "test-model",
            &OpenAiQualificationOptions {
                reasoning_effort: Some(OpenAiQualificationReasoningEffort::Disabled),
                ..OpenAiQualificationOptions::default()
            },
        )
        .await
        .expect("stability observation");
        assert_eq!(observation.attempts, 20);
        assert_eq!(observation.concurrency, 4);
        assert_eq!(
            observation.workload_revision,
            OPENAI_STABILITY_WORKLOAD_REVISION
        );
        assert_eq!(observation.total_prompt_tokens, 40);
        assert_eq!(observation.total_completion_tokens, 20);
        assert!(observation.p95_latency_ms <= observation.max_latency_ms);
        assert!(observation.wall_time_ms > 0);
        assert!(peak.load(Ordering::Acquire) >= 2);
        server.abort();
    }
}
