use crate::error::AppError;

pub(crate) fn validate_aibb_name(name: String) -> Result<String, AppError> {
    let name = name.trim();

    if name.is_empty() || name.chars().count() > 24 || name.chars().any(char::is_control) {
        return Err(AppError::new(
            "invalidProfile",
            "AIbb's name must be 1 to 24 characters and cannot contain control characters.",
        ));
    }

    Ok(name.to_string())
}
