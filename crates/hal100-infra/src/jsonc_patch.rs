use std::ops::Range;

use serde_json::Value;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum JsoncPatchError {
    #[error("configuration is not valid JSON or JSONC: {0}")]
    InvalidJson(String),
    #[error("configuration root must be an object")]
    RootMustBeObject,
    #[error("provider must be an object")]
    ProviderMustBeObject,
    #[error("provider.hal100 already exists and is not owned by HAL100")]
    ProviderConflict,
    #[error("configuration structure could not be located safely")]
    StructureNotFound,
}

pub struct ProviderPatch {
    pub output: String,
}

pub fn parse_jsonc(source: &str) -> Result<Value, JsoncPatchError> {
    let sanitized = sanitize_jsonc(source)?;
    serde_json::from_str(&sanitized)
        .map_err(|error| JsoncPatchError::InvalidJson(error.to_string()))
}

pub fn hal100_provider(value: &Value) -> Option<&Value> {
    value.get("provider")?.as_object()?.get("hal100")
}

pub fn patch_hal100_provider(
    source: &str,
    fragment: &Value,
    allow_replace: bool,
) -> Result<ProviderPatch, JsoncPatchError> {
    let value = parse_jsonc(source)?;
    if !value.is_object() {
        return Err(JsoncPatchError::RootMustBeObject);
    }
    if value
        .get("provider")
        .is_some_and(|provider| !provider.is_object())
    {
        return Err(JsoncPatchError::ProviderMustBeObject);
    }

    let sanitized = sanitize_jsonc(source)?;
    let root = root_object_span(&sanitized)?;
    let root_members = object_members(&sanitized, root.clone())?;
    let provider = root_members.iter().find(|member| member.key == "provider");

    match provider {
        Some(provider) => {
            if sanitized.as_bytes().get(provider.value.start) != Some(&b'{') {
                return Err(JsoncPatchError::ProviderMustBeObject);
            }
            let provider_members = object_members(&sanitized, provider.value.clone())?;
            if let Some(existing) = provider_members
                .iter()
                .find(|member| member.key == "hal100")
            {
                if !allow_replace {
                    return Err(JsoncPatchError::ProviderConflict);
                }
                Ok(ProviderPatch {
                    output: replace_value(source, existing.value.clone(), fragment)?,
                })
            } else {
                Ok(ProviderPatch {
                    output: insert_member(
                        source,
                        &sanitized,
                        provider.value.clone(),
                        "hal100",
                        fragment,
                    )?,
                })
            }
        }
        None => {
            let provider = serde_json::json!({ "hal100": fragment });
            Ok(ProviderPatch {
                output: insert_member(source, &sanitized, root, "provider", &provider)?,
            })
        }
    }
}

#[derive(Debug)]
struct ObjectMember {
    key: String,
    value: Range<usize>,
}

fn sanitize_jsonc(source: &str) -> Result<String, JsoncPatchError> {
    let bytes = source.as_bytes();
    let mut output = bytes.to_vec();
    let mut index = 0;
    let mut in_string = false;
    let mut escaped = false;
    while index < bytes.len() {
        if in_string {
            if escaped {
                escaped = false;
            } else if bytes[index] == b'\\' {
                escaped = true;
            } else if bytes[index] == b'"' {
                in_string = false;
            }
            index += 1;
            continue;
        }
        if bytes[index] == b'"' {
            in_string = true;
            index += 1;
            continue;
        }
        if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'/') {
            output[index] = b' ';
            output[index + 1] = b' ';
            index += 2;
            while index < bytes.len() && !matches!(bytes[index], b'\n' | b'\r') {
                output[index] = b' ';
                index += 1;
            }
            continue;
        }
        if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'*') {
            output[index] = b' ';
            output[index + 1] = b' ';
            index += 2;
            let mut closed = false;
            while index < bytes.len() {
                if bytes[index] == b'*' && bytes.get(index + 1) == Some(&b'/') {
                    output[index] = b' ';
                    output[index + 1] = b' ';
                    index += 2;
                    closed = true;
                    break;
                }
                if !matches!(bytes[index], b'\n' | b'\r') {
                    output[index] = b' ';
                }
                index += 1;
            }
            if !closed {
                return Err(JsoncPatchError::InvalidJson(
                    "unterminated block comment".to_owned(),
                ));
            }
            continue;
        }
        index += 1;
    }
    if in_string {
        return Err(JsoncPatchError::InvalidJson(
            "unterminated string".to_owned(),
        ));
    }

    let snapshot = output.clone();
    let mut index = 0;
    let mut in_string = false;
    let mut escaped = false;
    while index < snapshot.len() {
        if in_string {
            if escaped {
                escaped = false;
            } else if snapshot[index] == b'\\' {
                escaped = true;
            } else if snapshot[index] == b'"' {
                in_string = false;
            }
            index += 1;
            continue;
        }
        match snapshot[index] {
            b'"' => in_string = true,
            b',' => {
                let next = skip_whitespace(&snapshot, index + 1);
                if matches!(snapshot.get(next), Some(b'}' | b']')) {
                    output[index] = b' ';
                }
            }
            _ => {}
        }
        index += 1;
    }

    String::from_utf8(output)
        .map_err(|_| JsoncPatchError::InvalidJson("configuration is not UTF-8".to_owned()))
}

fn root_object_span(source: &str) -> Result<Range<usize>, JsoncPatchError> {
    let bytes = source.as_bytes();
    let start = skip_whitespace(bytes, 0);
    if bytes.get(start) != Some(&b'{') {
        return Err(JsoncPatchError::RootMustBeObject);
    }
    let end = skip_value(bytes, start)?;
    if skip_whitespace(bytes, end) != bytes.len() {
        return Err(JsoncPatchError::StructureNotFound);
    }
    Ok(start..end)
}

fn object_members(
    source: &str,
    object: Range<usize>,
) -> Result<Vec<ObjectMember>, JsoncPatchError> {
    let bytes = source.as_bytes();
    if bytes.get(object.start) != Some(&b'{') || bytes.get(object.end - 1) != Some(&b'}') {
        return Err(JsoncPatchError::StructureNotFound);
    }
    let mut index = object.start + 1;
    let mut members = Vec::new();
    loop {
        index = skip_whitespace(bytes, index);
        if bytes.get(index) == Some(&b'}') {
            break;
        }
        if bytes.get(index) != Some(&b'"') {
            return Err(JsoncPatchError::StructureNotFound);
        }
        let key_end = skip_string(bytes, index)?;
        let key: String = serde_json::from_str(&source[index..key_end])
            .map_err(|_| JsoncPatchError::StructureNotFound)?;
        index = skip_whitespace(bytes, key_end);
        if bytes.get(index) != Some(&b':') {
            return Err(JsoncPatchError::StructureNotFound);
        }
        let value_start = skip_whitespace(bytes, index + 1);
        let value_end = skip_value(bytes, value_start)?;
        members.push(ObjectMember {
            key,
            value: value_start..value_end,
        });
        index = skip_whitespace(bytes, value_end);
        match bytes.get(index) {
            Some(b',') => index += 1,
            Some(b'}') => break,
            _ => return Err(JsoncPatchError::StructureNotFound),
        }
    }
    Ok(members)
}

fn skip_value(bytes: &[u8], start: usize) -> Result<usize, JsoncPatchError> {
    match bytes.get(start) {
        Some(b'"') => skip_string(bytes, start),
        Some(b'{') | Some(b'[') => {
            let open = bytes[start];
            let close = if open == b'{' { b'}' } else { b']' };
            let mut depth = 1usize;
            let mut index = start + 1;
            while index < bytes.len() {
                match bytes[index] {
                    b'"' => index = skip_string(bytes, index)?,
                    byte if byte == open => {
                        depth += 1;
                        index += 1;
                    }
                    byte if byte == close => {
                        depth -= 1;
                        index += 1;
                        if depth == 0 {
                            return Ok(index);
                        }
                    }
                    _ => index += 1,
                }
            }
            Err(JsoncPatchError::StructureNotFound)
        }
        Some(_) => {
            let mut index = start;
            while index < bytes.len()
                && !matches!(
                    bytes[index],
                    b',' | b'}' | b']' | b' ' | b'\t' | b'\r' | b'\n'
                )
            {
                index += 1;
            }
            (index > start)
                .then_some(index)
                .ok_or(JsoncPatchError::StructureNotFound)
        }
        None => Err(JsoncPatchError::StructureNotFound),
    }
}

fn skip_string(bytes: &[u8], start: usize) -> Result<usize, JsoncPatchError> {
    let mut index = start + 1;
    let mut escaped = false;
    while index < bytes.len() {
        if escaped {
            escaped = false;
        } else if bytes[index] == b'\\' {
            escaped = true;
        } else if bytes[index] == b'"' {
            return Ok(index + 1);
        }
        index += 1;
    }
    Err(JsoncPatchError::StructureNotFound)
}

fn skip_whitespace(bytes: &[u8], mut index: usize) -> usize {
    while matches!(bytes.get(index), Some(b' ' | b'\t' | b'\r' | b'\n')) {
        index += 1;
    }
    index
}

fn replace_value(
    source: &str,
    range: Range<usize>,
    value: &Value,
) -> Result<String, JsoncPatchError> {
    let indent = line_indent(source, range.start);
    let replacement = formatted_value(value, &indent)?;
    Ok(format!(
        "{}{}{}",
        &source[..range.start],
        replacement,
        &source[range.end..]
    ))
}

fn insert_member(
    source: &str,
    sanitized: &str,
    object: Range<usize>,
    key: &str,
    value: &Value,
) -> Result<String, JsoncPatchError> {
    let members = object_members(sanitized, object.clone())?;
    let base_indent = line_indent(source, object.start);
    let child_indent = format!("{base_indent}  ");
    let formatted = formatted_value(value, &child_indent)?;
    let close = object.end - 1;

    let (insert_at, comma) = if let Some(last) = members.last() {
        let next = skip_whitespace(sanitized.as_bytes(), last.value.end);
        if sanitized.as_bytes().get(next) == Some(&b',') {
            (next + 1, "")
        } else {
            (last.value.end, ",")
        }
    } else {
        (object.start + 1, "")
    };
    let trailing = &source[insert_at..close];
    let closing_newline = if trailing.contains('\n') {
        String::new()
    } else {
        format!("\n{base_indent}")
    };
    let insertion = format!("{comma}\n{child_indent}\"{key}\": {formatted}{closing_newline}");
    Ok(format!(
        "{}{}{}",
        &source[..insert_at],
        insertion,
        &source[insert_at..]
    ))
}

fn formatted_value(value: &Value, continuation_indent: &str) -> Result<String, JsoncPatchError> {
    let pretty = serde_json::to_string_pretty(value)
        .map_err(|error| JsoncPatchError::InvalidJson(error.to_string()))?;
    let mut lines = pretty.lines();
    let mut output = lines.next().unwrap_or_default().to_owned();
    for line in lines {
        output.push('\n');
        output.push_str(continuation_indent);
        output.push_str(line);
    }
    Ok(output)
}

fn line_indent(source: &str, index: usize) -> String {
    let line_start = source[..index]
        .rfind('\n')
        .map_or(0, |position| position + 1);
    source[line_start..]
        .chars()
        .take_while(|character| matches!(character, ' ' | '\t'))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fragment() -> Value {
        serde_json::json!({
            "npm": "@ai-sdk/openai-compatible",
            "name": "HAL100 · 由 HAL100 管理",
            "options": { "baseURL": "http://127.0.0.1:10100/v1" },
            "models": { "hal100-active": { "name": "HAL100 当前模型" } }
        })
    }

    #[test]
    fn inserts_provider_without_reformatting_unknown_jsonc() {
        let source = "{\n  // keep this comment\n  \"mcp\": {\"demo\": true},\n}\n";
        let patched = patch_hal100_provider(source, &fragment(), false).expect("patch");
        let parsed = parse_jsonc(&patched.output).expect("parse patched config");

        assert!(patched.output.contains("// keep this comment"));
        assert_eq!(parsed["mcp"]["demo"], true);
        assert_eq!(
            parsed["provider"]["hal100"]["name"],
            "HAL100 · 由 HAL100 管理"
        );
        assert!(parsed.get("model").is_none());
    }

    #[test]
    fn preserves_existing_providers_and_default_model() {
        let source = r#"{
  "model": "anthropic/existing",
  "provider": {
    "existing": { "options": { "apiKey": "do-not-touch" } }
  }
}"#;
        let patched = patch_hal100_provider(source, &fragment(), false).expect("patch");
        let parsed = parse_jsonc(&patched.output).expect("parse patched config");

        assert_eq!(parsed["model"], "anthropic/existing");
        assert_eq!(
            parsed["provider"]["existing"]["options"]["apiKey"],
            "do-not-touch"
        );
    }

    #[test]
    fn refuses_to_replace_an_unowned_provider() {
        let source = r#"{"provider":{"hal100":{"name":"someone else"}}}"#;
        assert!(matches!(
            patch_hal100_provider(source, &fragment(), false),
            Err(JsoncPatchError::ProviderConflict)
        ));
    }

    #[test]
    fn replaces_only_the_owned_provider_value() {
        let source = r#"{
  "unknown": 42,
  "provider": {
    "hal100": { "name": "old" },
    "other": { "name": "keep" }
  }
}"#;
        let patched = patch_hal100_provider(source, &fragment(), true).expect("replace");
        let parsed = parse_jsonc(&patched.output).expect("parse patched config");

        assert_eq!(parsed["unknown"], 42);
        assert_eq!(parsed["provider"]["other"]["name"], "keep");
        assert_eq!(
            parsed["provider"]["hal100"]["name"],
            "HAL100 · 由 HAL100 管理"
        );
    }
}
