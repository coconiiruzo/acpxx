//! Small, dependency-free safeguards for text that may cross a diagnostic boundary.

const ASSIGNMENT_MARKERS: &[&str] = &[
    "OPENAI_API_KEY=",
    "CODEX_API_KEY=",
    "ANTHROPIC_API_KEY=",
    "CLAUDE_CODE_OAUTH_TOKEN=",
    "XAI_API_KEY=",
    "CURSOR_API_KEY=",
    "Authorization: Bearer ",
];

const JSON_MARKERS: &[&str] = &[
    "\"api_key\":",
    "\"access_token\":",
    "\"oauth_token\":",
    "\"authorization\":",
];

/// Redacts common credential forms before provider-controlled text is logged or persisted.
#[must_use]
pub fn redact_sensitive(input: &str) -> String {
    let mut output = input.to_owned();
    for marker in ASSIGNMENT_MARKERS {
        redact_after_marker(&mut output, marker);
    }
    for marker in JSON_MARKERS {
        redact_after_marker(&mut output, marker);
    }
    output
}

/// Redacts sensitive strings recursively without changing a metadata value's shape.
pub fn redact_json_sensitive(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::String(text) => *text = redact_sensitive(text),
        serde_json::Value::Array(values) => {
            for value in values {
                redact_json_sensitive(value);
            }
        }
        serde_json::Value::Object(values) => {
            for (key, value) in values {
                let normalized = key.to_ascii_lowercase();
                if matches!(
                    normalized.as_str(),
                    "api_key"
                        | "apikey"
                        | "access_token"
                        | "oauth_token"
                        | "authorization"
                        | "password"
                        | "secret"
                ) {
                    *value = serde_json::Value::String("[REDACTED]".into());
                } else {
                    redact_json_sensitive(value);
                }
            }
        }
        _ => {}
    }
}

fn redact_after_marker(text: &mut String, marker: &str) {
    let mut cursor = 0;
    loop {
        let lowercase = text[cursor..].to_ascii_lowercase();
        let Some(relative) = lowercase.find(&marker.to_ascii_lowercase()) else {
            return;
        };
        let marker_end = cursor + relative + marker.len();
        let bytes = text.as_bytes();
        let mut value_start = marker_end;
        while bytes
            .get(value_start)
            .is_some_and(|byte| byte.is_ascii_whitespace() || matches!(byte, b'\'' | b'\"'))
        {
            value_start += 1;
        }
        let mut value_end = value_start;
        while bytes.get(value_end).is_some_and(|byte| {
            !byte.is_ascii_whitespace() && !matches!(byte, b'\'' | b'\"' | b',' | b';' | b'}')
        }) {
            value_end += 1;
        }
        if value_end > value_start {
            text.replace_range(value_start..value_end, "[REDACTED]");
            cursor = value_start + "[REDACTED]".len();
        } else {
            cursor = marker_end;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_assignments_bearer_headers_and_json_values() {
        let input = concat!(
            "OPENAI_API_KEY=sk-live-secret ",
            "Authorization: Bearer bearer-secret ",
            "{\"access_token\":\"oauth-secret\",\"safe\":\"visible\"}"
        );
        let redacted = redact_sensitive(input);
        assert!(!redacted.contains("sk-live-secret"));
        assert!(!redacted.contains("bearer-secret"));
        assert!(!redacted.contains("oauth-secret"));
        assert!(redacted.contains("visible"));
        assert_eq!(redacted.matches("[REDACTED]").count(), 3);
    }

    #[test]
    fn matching_is_case_insensitive() {
        assert_eq!(
            redact_sensitive("anthropic_api_key='secret'"),
            "anthropic_api_key='[REDACTED]'"
        );
    }

    #[test]
    fn json_redaction_preserves_shape_and_removes_secret_values() {
        let mut value = serde_json::json!({
            "nested": {"access_token": "token", "label": "safe"},
            "message": "OPENAI_API_KEY=secret"
        });
        redact_json_sensitive(&mut value);
        assert_eq!(value["nested"]["access_token"], "[REDACTED]");
        assert_eq!(value["nested"]["label"], "safe");
        assert_eq!(value["message"], "OPENAI_API_KEY=[REDACTED]");
    }
}
