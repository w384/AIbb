#[derive(Debug, serde::Serialize, thiserror::Error)]
#[serde(rename_all = "camelCase")]
#[error("{message}")]
pub struct AppError {
    pub code: String,
    pub message: String,
}

impl AppError {
    pub fn sanitized(
        code: impl Into<String>,
        message: impl AsRef<str>,
        current_key: Option<&str>,
    ) -> Self {
        let code = code.into();
        let mut safe_code = redact_authorization_values(&code);
        let mut safe_message = message
            .as_ref()
            .lines()
            .map(redact_authorization_values)
            .collect::<Vec<_>>()
            .join("\n");

        if let Some(api_key) = current_key.filter(|key| !key.is_empty()) {
            safe_code = safe_code.replace(api_key, "[REDACTED]");
            safe_message = safe_message.replace(api_key, "[REDACTED]");
        }

        Self {
            code: safe_code,
            message: safe_message,
        }
    }
}

fn redact_authorization_values(value: &str) -> String {
    let lowercase = value.to_ascii_lowercase();
    let Some(name_start) = lowercase.find("authorization") else {
        return value.to_string();
    };
    let name_end = name_start + "authorization".len();
    let Some((separator_offset, _)) = value[name_end..]
        .char_indices()
        .find(|(_, character)| matches!(character, ':' | '='))
    else {
        return value.to_string();
    };
    let value_start = name_end + separator_offset + 1;
    let remainder = &value[value_start..];
    let trimmed = remainder.trim_start();
    let suffix = match trimmed.chars().next() {
        Some(quote @ ('"' | '\'')) => trimmed[quote.len_utf8()..]
            .find(quote)
            .map(|closing_offset| {
                let after_quote = quote.len_utf8() + closing_offset + quote.len_utf8();
                redact_authorization_values(&trimmed[after_quote..])
            })
            .unwrap_or_default(),
        _ => String::new(),
    };

    format!(
        "{}Authorization: [REDACTED]{}",
        &value[..name_start],
        suffix
    )
}

#[cfg(test)]
mod tests {
    use super::AppError;

    #[test]
    fn redacts_authorization_headers_and_the_current_key_before_serialization() {
        let error = AppError::sanitized(
            "connectionFailed",
            "request failed\nAuthorization: Bearer sk-current\nkey=sk-current",
            Some("sk-current"),
        );

        let displayed = error.to_string();
        let serialized = serde_json::to_string(&error).unwrap();

        assert!(!displayed.contains("Bearer sk-current"));
        assert!(!displayed.contains("key=sk-current"));
        assert!(!serialized.contains("sk-current"));
        assert!(displayed.contains("Authorization: [REDACTED]"));
    }

    #[test]
    fn redacts_code_and_inline_or_structured_authorization_values() {
        let error = AppError::sanitized(
            "credential-sk-current",
            "request aUtHoRiZaTiOn = Bearer inline-secret\n\
             payload={\"AUTHORIZATION\":\"Bearer structured-secret\"}",
            Some("sk-current"),
        );

        let serialized = serde_json::to_string(&error).unwrap();

        assert!(!serialized.contains("sk-current"));
        assert!(!serialized.contains("inline-secret"));
        assert!(!serialized.contains("structured-secret"));
        assert!(serialized.matches("[REDACTED]").count() >= 3);
    }
}
