use crate::error::AppError;

/// Opens a link in the user's default browser. Left-clicking any link in the
/// chat calls this command; the webview never navigates itself.
#[tauri::command]
pub fn open_external_url(url: String) -> Result<(), AppError> {
    let validated = validate_external_url(&url)?;
    opener::open_browser(&validated)
        .map_err(|error| AppError::new("open_external_url_failed", &format!("Failed to open the link: {error}")))
}

/// Only http(s) links may be handed to the system browser; anything else
/// (file:, javascript:, …) is rejected so the opener never receives a
/// dangerous target.
pub fn validate_external_url(url: &str) -> Result<String, AppError> {
    let parsed = url::Url::parse(url.trim())
        .map_err(|_| AppError::new("invalid_external_url", "The link is not a valid URL."))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(AppError::new(
            "invalid_external_url",
            "Only http and https links can be opened.",
        ));
    }
    Ok(parsed.to_string())
}

#[cfg(test)]
mod tests {
    use super::validate_external_url;

    #[test]
    fn http_and_https_links_are_allowed() {
        assert_eq!(
            validate_external_url("https://example.com/甲?q=1#frag").unwrap(),
            "https://example.com/%E7%94%B2?q=1#frag"
        );
        assert_eq!(
            validate_external_url("  http://example.com/  ").unwrap(),
            "http://example.com/"
        );
    }

    #[test]
    fn non_http_schemes_and_garbage_are_rejected() {
        for raw in [
            "file:///C:/Windows/notepad.exe",
            "javascript:alert(1)",
            "data:text/html,<script>",
            "not a url",
            "",
        ] {
            assert!(validate_external_url(raw).is_err(), "raw: {raw}");
        }
    }
}
