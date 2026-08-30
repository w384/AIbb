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
        let mut safe_message = message
            .as_ref()
            .lines()
            .map(|line| {
                let trimmed = line.trim_start();
                match trimmed.split_once(':') {
                    Some((name, _)) if name.trim().eq_ignore_ascii_case("authorization") => {
                        "Authorization: [REDACTED]"
                    }
                    _ => line,
                }
            })
            .collect::<Vec<_>>()
            .join("\n");

        if let Some(api_key) = current_key.filter(|key| !key.is_empty()) {
            safe_message = safe_message.replace(api_key, "[REDACTED]");
        }

        Self {
            code: code.into(),
            message: safe_message,
        }
    }
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
}
