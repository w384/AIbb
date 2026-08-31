#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    AuthenticationFailed,
    ModelNotFound,
    RateLimited,
    RequestTimeout,
    Cancelled,
    ProviderUnavailable,
    InvalidRequest,
    InvalidResponse,
    UnsafeUrl,
    UnsupportedContent,
    ResponseTooLarge,
    PublicSearchUnavailable,
    PublicPageUnavailable,
    RedirectLimitExceeded,
    PageBudgetExceeded,
}

impl ErrorCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AuthenticationFailed => "authentication_failed",
            Self::ModelNotFound => "model_not_found",
            Self::RateLimited => "rate_limited",
            Self::RequestTimeout => "request_timeout",
            Self::Cancelled => "cancelled",
            Self::ProviderUnavailable => "provider_unavailable",
            Self::InvalidRequest => "invalid_request",
            Self::InvalidResponse => "invalid_response",
            Self::UnsafeUrl => "unsafe_url",
            Self::UnsupportedContent => "unsupported_content",
            Self::ResponseTooLarge => "response_too_large",
            Self::PublicSearchUnavailable => "public_search_unavailable",
            Self::PublicPageUnavailable => "public_page_unavailable",
            Self::RedirectLimitExceeded => "redirect_limit_exceeded",
            Self::PageBudgetExceeded => "page_budget_exceeded",
        }
    }

    const fn public_message(self) -> &'static str {
        match self {
            Self::AuthenticationFailed => "The model provider rejected the API credential.",
            Self::ModelNotFound => "The configured model was not found.",
            Self::RateLimited => "The model provider rate limit was reached.",
            Self::RequestTimeout => "The model request timed out.",
            Self::Cancelled => "The model request was cancelled.",
            Self::ProviderUnavailable => "The model provider is unavailable.",
            Self::InvalidRequest => "The model provider rejected the request.",
            Self::InvalidResponse => "The model provider returned an invalid response.",
            Self::UnsafeUrl => "The public page address is not safe to access.",
            Self::UnsupportedContent => "The public page content type is not supported.",
            Self::ResponseTooLarge => "The public page response is too large.",
            Self::PublicSearchUnavailable => "Public search is temporarily unavailable.",
            Self::PublicPageUnavailable => "The public page is temporarily unavailable.",
            Self::RedirectLimitExceeded => "The public page redirected too many times.",
            Self::PageBudgetExceeded => "The public page exploration limit was reached.",
        }
    }
}

#[derive(Debug, serde::Serialize, thiserror::Error)]
#[serde(rename_all = "camelCase")]
#[error("{message}")]
pub struct AppError {
    pub code: String,
    pub message: String,
    #[serde(skip)]
    diagnostic: Option<String>,
}

impl AppError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            diagnostic: None,
        }
    }

    pub fn from_code(code: ErrorCode) -> Self {
        Self::new(code.as_str(), code.public_message())
    }

    pub fn from_http_body(
        status: u16,
        body: &str,
        current_key: Option<&str>,
        chat_or_model_endpoint: bool,
    ) -> Self {
        let code = match status {
            401 | 403 => ErrorCode::AuthenticationFailed,
            404 if chat_or_model_endpoint => ErrorCode::ModelNotFound,
            429 => ErrorCode::RateLimited,
            500..=599 => ErrorCode::ProviderUnavailable,
            _ => ErrorCode::InvalidRequest,
        };
        let redacted_body = redact_secret(body, current_key);

        Self {
            code: code.as_str().to_owned(),
            message: code.public_message().to_owned(),
            diagnostic: Some(format!("provider HTTP {status}: {redacted_body}")),
        }
    }

    pub fn with_diagnostic(
        mut self,
        diagnostic: impl AsRef<str>,
        current_key: Option<&str>,
    ) -> Self {
        self.diagnostic = Some(redact_secret(diagnostic.as_ref(), current_key));
        self
    }

    pub fn diagnostic(&self) -> Option<&str> {
        self.diagnostic.as_deref()
    }

    pub fn sanitized(
        code: impl Into<String>,
        message: impl AsRef<str>,
        current_key: Option<&str>,
    ) -> Self {
        let code = code.into();
        Self::new(
            redact_secret(&code, current_key),
            redact_secret(message.as_ref(), current_key),
        )
    }
}

fn redact_secret(value: &str, current_key: Option<&str>) -> String {
    let mut safe = value
        .lines()
        .map(redact_authorization_values)
        .collect::<Vec<_>>()
        .join("\n");

    if let Some(api_key) = current_key.filter(|key| !key.is_empty()) {
        safe = safe.replace(api_key, "[REDACTED]");
    }

    safe
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
