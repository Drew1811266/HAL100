use hal100_protocol::{ExternalAgentInputModality, ExternalAgentModelProfile};
use serde_json::{Map, Value, json};
use thiserror::Error;

pub(crate) const HERMES_PROVIDER_KEY: &str = "hal100";
pub(crate) const HERMES_CREDENTIAL_ENV_KEY: &str = "HAL100_HERMES_GATEWAY_KEY";

#[derive(Debug, Error)]
pub(crate) enum HermesConfigError {
    #[error("Hermes config.yaml不是UTF-8文本")]
    InvalidConfigUtf8,
    #[error("Hermes config.yaml不是有效YAML: {0}")]
    InvalidYaml(String),
    #[error("Hermes config.yaml根节点必须是对象")]
    RootMustBeObject,
    #[error("Hermes config.yaml中的providers必须是对象")]
    ProvidersMustBeObject,
    #[error("Hermes现有providers.hal100不属于HAL100，已拒绝覆盖")]
    ProviderConflict,
    #[error("Hermes .env不是UTF-8文本")]
    InvalidEnvironmentUtf8,
    #[error("Hermes .env中存在多个HAL100专属变量")]
    DuplicateCredentialVariable,
    #[error("Hermes .env中的HAL100专属变量不属于当前接入，已拒绝覆盖")]
    CredentialVariableConflict,
}

pub(crate) fn parse_yaml_config(bytes: &[u8]) -> Result<Value, HermesConfigError> {
    if bytes.is_empty() || bytes.iter().all(u8::is_ascii_whitespace) {
        return Ok(Value::Object(Map::new()));
    }
    let source = std::str::from_utf8(bytes).map_err(|_| HermesConfigError::InvalidConfigUtf8)?;
    let value: Value = serde_yaml_ng::from_str(source)
        .map_err(|error| HermesConfigError::InvalidYaml(error.to_string()))?;
    if value.is_null() {
        return Ok(Value::Object(Map::new()));
    }
    if !value.is_object() {
        return Err(HermesConfigError::RootMustBeObject);
    }
    Ok(value)
}

pub(crate) fn serialize_yaml_config(value: &Value) -> Result<Vec<u8>, HermesConfigError> {
    if !value.is_object() {
        return Err(HermesConfigError::RootMustBeObject);
    }
    let mut source = serde_yaml_ng::to_string(value)
        .map_err(|error| HermesConfigError::InvalidYaml(error.to_string()))?;
    if !source.ends_with('\n') {
        source.push('\n');
    }
    Ok(source.into_bytes())
}

pub(crate) fn managed_provider(config: &Value) -> Result<Option<&Value>, HermesConfigError> {
    let root = config
        .as_object()
        .ok_or(HermesConfigError::RootMustBeObject)?;
    let Some(providers) = root.get("providers") else {
        return Ok(None);
    };
    let providers = providers
        .as_object()
        .ok_or(HermesConfigError::ProvidersMustBeObject)?;
    Ok(providers.get(HERMES_PROVIDER_KEY))
}

pub(crate) fn patch_managed_provider(
    config: &Value,
    fragment: &Value,
    allow_replace: bool,
) -> Result<Value, HermesConfigError> {
    let mut output = config.clone();
    let root = output
        .as_object_mut()
        .ok_or(HermesConfigError::RootMustBeObject)?;
    if !root.contains_key("providers") {
        root.insert("providers".to_owned(), Value::Object(Map::new()));
    }
    let providers = root
        .get_mut("providers")
        .and_then(Value::as_object_mut)
        .ok_or(HermesConfigError::ProvidersMustBeObject)?;
    if providers.contains_key(HERMES_PROVIDER_KEY) && !allow_replace {
        return Err(HermesConfigError::ProviderConflict);
    }
    providers.insert(HERMES_PROVIDER_KEY.to_owned(), fragment.clone());
    Ok(output)
}

pub(crate) fn remove_managed_provider(config: &Value) -> Result<Value, HermesConfigError> {
    let mut output = config.clone();
    let root = output
        .as_object_mut()
        .ok_or(HermesConfigError::RootMustBeObject)?;
    let providers = root
        .get_mut("providers")
        .and_then(Value::as_object_mut)
        .ok_or(HermesConfigError::ProvidersMustBeObject)?;
    providers.remove(HERMES_PROVIDER_KEY);
    Ok(output)
}

pub(crate) fn provider_fragment(
    gateway_base_url: &str,
    profile: &ExternalAgentModelProfile,
) -> Value {
    let supports_vision = profile
        .input_modalities
        .contains(&ExternalAgentInputModality::Image);
    json!({
        "name": "HAL100",
        "api": gateway_base_url,
        "key_env": HERMES_CREDENTIAL_ENV_KEY,
        "transport": "chat_completions",
        "default_model": profile.model_id,
        "discover_models": false,
        "context_length": profile.context_window_tokens,
        "models": {
            profile.model_id.clone(): {
                "context_length": profile.context_window_tokens,
                "supports_vision": supports_vision
            }
        }
    })
}

pub(crate) fn read_managed_env_value(bytes: &[u8]) -> Result<Option<String>, HermesConfigError> {
    let source =
        std::str::from_utf8(bytes).map_err(|_| HermesConfigError::InvalidEnvironmentUtf8)?;
    let matches = credential_lines(source);
    if matches.len() > 1 {
        return Err(HermesConfigError::DuplicateCredentialVariable);
    }
    Ok(matches.first().map(|line| line.value.to_owned()))
}

pub(crate) fn patch_managed_env(
    bytes: &[u8],
    plaintext_key: &str,
    allow_replace: bool,
) -> Result<Vec<u8>, HermesConfigError> {
    let source =
        std::str::from_utf8(bytes).map_err(|_| HermesConfigError::InvalidEnvironmentUtf8)?;
    let matches = credential_lines(source);
    if matches.len() > 1 {
        return Err(HermesConfigError::DuplicateCredentialVariable);
    }
    let replacement = format!("{HERMES_CREDENTIAL_ENV_KEY}={plaintext_key}");
    if let Some(line) = matches.first() {
        if !allow_replace {
            return Err(HermesConfigError::CredentialVariableConflict);
        }
        let mut output = String::with_capacity(source.len() + replacement.len());
        output.push_str(&source[..line.start]);
        output.push_str(&replacement);
        output.push_str(&source[line.end..]);
        return Ok(output.into_bytes());
    }

    let mut output = source.to_owned();
    if !output.is_empty() && !output.ends_with('\n') {
        output.push('\n');
    }
    output.push_str(&replacement);
    output.push('\n');
    Ok(output.into_bytes())
}

pub(crate) fn remove_managed_env(bytes: &[u8]) -> Result<Vec<u8>, HermesConfigError> {
    let source =
        std::str::from_utf8(bytes).map_err(|_| HermesConfigError::InvalidEnvironmentUtf8)?;
    let matches = credential_lines(source);
    if matches.len() > 1 {
        return Err(HermesConfigError::DuplicateCredentialVariable);
    }
    let Some(line) = matches.first() else {
        return Ok(bytes.to_vec());
    };
    let mut output = String::with_capacity(source.len());
    output.push_str(&source[..line.start]);
    output.push_str(&source[line.end_with_newline..]);
    Ok(output.into_bytes())
}

pub(crate) fn env_entry_is_canonical(bytes: &[u8], plaintext_key: &str) -> bool {
    let Ok(source) = std::str::from_utf8(bytes) else {
        return false;
    };
    let matches = credential_lines(source);
    matches.len() == 1 && matches[0].raw == format!("{HERMES_CREDENTIAL_ENV_KEY}={plaintext_key}")
}

struct CredentialLine<'a> {
    start: usize,
    end: usize,
    end_with_newline: usize,
    raw: &'a str,
    value: &'a str,
}

fn credential_lines(source: &str) -> Vec<CredentialLine<'_>> {
    let mut matches = Vec::new();
    let mut offset = 0;
    for inclusive in source.split_inclusive('\n') {
        let line = inclusive.strip_suffix('\n').unwrap_or(inclusive);
        let line = line.strip_suffix('\r').unwrap_or(line);
        if let Some(value) = credential_value(line) {
            matches.push(CredentialLine {
                start: offset,
                end: offset + line.len(),
                end_with_newline: offset + inclusive.len(),
                raw: line,
                value,
            });
        }
        offset += inclusive.len();
    }
    if source.is_empty() || source.ends_with('\n') {
        return matches;
    }
    matches
}

fn credential_value(line: &str) -> Option<&str> {
    let candidate = line.trim_start();
    let candidate = candidate.strip_prefix("export ").unwrap_or(candidate);
    let (name, value) = candidate.split_once('=')?;
    (name.trim() == HERMES_CREDENTIAL_ENV_KEY).then(|| value.trim())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ExternalModelProfileRegistry;

    #[test]
    fn yaml_patch_preserves_other_providers_and_default_model() {
        let original = parse_yaml_config(
            b"model:\n  default: personal-model\nproviders:\n  personal:\n    api: https://example.test/v1\n",
        )
        .expect("parse");
        let profile = ExternalModelProfileRegistry::conservative_managed_route()
            .snapshot()
            .expect("profile");
        let fragment = provider_fragment("http://127.0.0.1:10100/v1", &profile);
        let patched = patch_managed_provider(&original, &fragment, false).expect("patch");
        let reparsed = parse_yaml_config(&serialize_yaml_config(&patched).expect("serialize"))
            .expect("reparse");

        assert_eq!(reparsed["model"]["default"], "personal-model");
        assert_eq!(
            reparsed["providers"]["personal"]["api"],
            "https://example.test/v1"
        );
        assert_eq!(
            reparsed["providers"]["hal100"]["transport"],
            "chat_completions"
        );
        assert_eq!(
            reparsed["providers"]["hal100"]["models"]["hal100-active"]["context_length"],
            crate::AGENT_BASELINE_CONTEXT_WINDOW_TOKENS
        );
    }

    #[test]
    fn dotenv_patch_and_remove_preserve_every_other_line() {
        let original = b"# personal\nOPENROUTER_API_KEY=secret\nLAST=value";
        let patched = patch_managed_env(original, "hal100_secret", false).expect("patch");
        assert_eq!(
            std::str::from_utf8(&patched).expect("utf8"),
            "# personal\nOPENROUTER_API_KEY=secret\nLAST=value\nHAL100_HERMES_GATEWAY_KEY=hal100_secret\n"
        );
        assert_eq!(
            read_managed_env_value(&patched).expect("read"),
            Some("hal100_secret".to_owned())
        );
        assert!(env_entry_is_canonical(&patched, "hal100_secret"));
        assert_eq!(
            remove_managed_env(&patched).expect("remove"),
            b"# personal\nOPENROUTER_API_KEY=secret\nLAST=value\n"
        );
    }

    #[test]
    fn existing_foreign_provider_and_env_variable_are_conflicts() {
        let config = parse_yaml_config(b"providers:\n  hal100:\n    api: https://foreign.test\n")
            .expect("parse");
        assert!(matches!(
            patch_managed_provider(&config, &json!({}), false),
            Err(HermesConfigError::ProviderConflict)
        ));
        assert!(matches!(
            patch_managed_env(b"HAL100_HERMES_GATEWAY_KEY=foreign\n", "new", false),
            Err(HermesConfigError::CredentialVariableConflict)
        ));
    }
}
