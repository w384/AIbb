use serde::Deserialize;
use tokio_util::sync::CancellationToken;

use crate::{
    error::AppError,
    exploration::UserInputIntent,
    llm::{ChatMessage, ChatRequest, LlmTransport},
};

/// A single lightweight model call decides whether the user wants AIbb to go
/// out exploring (and in which direction) or is just chatting. Keyword routing
/// only remains as a fast path for explicitly-shaped requests; everything
/// else is judged by the model, so natural wording like 「出发出发」 works.
const INTENT_CLASSIFICATION_INSTRUCTION: &str = "判断这条用户消息的意图，只输出一个 JSON 对象，不要输出任何其他内容：{\"intent\":\"chat\"或\"explore\",\"direction\":字符串或null}\n\n- \"explore\"：用户明确让 AIbb 出去探索、出去玩、去逛逛、去看看新鲜事、去搜罗信息，或明确给出了想探索的方向或主题（如“去海边玩”“讲讲量子计算的最新进展”“看看最近有什么新发现”）。direction 填用户明确给出的方向或主题；用户只说“出去玩”“出发”“去逛逛”这类没有具体方向的话时，direction 填 null。\n- \"chat\"：普通聊天、寒暄、问候、情感倾诉、求助、日常问答，或只是提到玩但并不打算让 AIbb 现在出发（如“昨天我去公园玩了”）。";

pub async fn classify_user_intent(
    llm: &dyn LlmTransport,
    message: &str,
) -> Option<UserInputIntent> {
    let request = ChatRequest {
        messages: vec![
            ChatMessage::new("system", INTENT_CLASSIFICATION_INSTRUCTION),
            ChatMessage::user(message),
        ],
    };
    let raw = llm.complete(request, CancellationToken::new()).await.ok()?;
    parse_intent_classification(&raw).ok()
}

fn parse_intent_classification(raw: &str) -> Result<UserInputIntent, AppError> {
    let payload = extract_json_payload(raw).ok_or_else(intent_error)?;
    let envelope: IntentEnvelope =
        serde_json::from_str(payload).map_err(|_| intent_error())?;
    match envelope.intent.trim() {
        "explore" => Ok(UserInputIntent::Explore {
            direction: envelope
                .direction
                .map(|direction| direction.trim().to_string())
                .filter(|direction| !direction.is_empty())
                .filter(|direction| is_safe_direction(direction)),
        }),
        "chat" => Ok(UserInputIntent::Chat),
        _ => Err(intent_error()),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IntentEnvelope {
    intent: String,
    #[serde(default)]
    direction: Option<String>,
}

fn extract_json_payload(raw: &str) -> Option<&str> {
    let trimmed = raw.trim();
    if !trimmed.contains("```") {
        return Some(trimmed);
    }

    let opening_marker = trimmed
        .find("```json")
        .or_else(|| trimmed.find("```"))?;
    let after_marker = &trimmed[opening_marker..];
    let opening_end = after_marker.find('\n')?;
    let body = &after_marker[opening_end + 1..];
    let closing = body.rfind("```")?;
    Some(body[..closing].trim())
}

fn is_safe_direction(direction: &str) -> bool {
    direction.chars().count() <= 200
        && !direction
            .chars()
            .any(|character| character.is_control() || character.is_whitespace() && character != ' ')
}

fn intent_error() -> AppError {
    AppError::new(
        "invalid_intent_classification",
        "The model returned an invalid intent classification.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_an_explore_decision_without_direction() {
        let intent = parse_intent_classification(r#"{"intent":"explore","direction":null}"#).unwrap();
        assert_eq!(intent, UserInputIntent::Explore { direction: None });
    }

    #[test]
    fn parses_an_explore_decision_with_a_direction() {
        let intent = parse_intent_classification(
            r#"{"intent":"explore","direction":"  去海边  "}"#,
        )
        .unwrap();
        assert_eq!(
            intent,
            UserInputIntent::Explore {
                direction: Some("去海边".into()),
            }
        );
    }

    #[test]
    fn parses_a_fenced_json_explore_decision() {
        let intent = parse_intent_classification(
            "好的，我的判断如下：\n```json\n{\"intent\":\"explore\",\"direction\":null}\n```\n",
        )
        .unwrap();
        assert_eq!(intent, UserInputIntent::Explore { direction: None });
    }

    #[test]
    fn parses_a_chat_decision() {
        let intent =
            parse_intent_classification(r#"{"intent":"chat","direction":null}"#).unwrap();
        assert_eq!(intent, UserInputIntent::Chat);
    }

    #[test]
    fn rejects_unknown_intents_and_oversized_directions() {
        assert!(parse_intent_classification(r#"{"intent":"dance"}"#).is_err());
        assert!(parse_intent_classification("不是 JSON").is_err());
        let oversized = format!(
            r#"{{"intent":"explore","direction":"{}"}}"#,
            "a".repeat(201)
        );
        let intent = parse_intent_classification(&oversized).unwrap();
        assert_eq!(intent, UserInputIntent::Explore { direction: None });
    }

    #[test]
    fn empty_or_whitespace_direction_becomes_no_direction() {
        let intent =
            parse_intent_classification(r#"{"intent":"explore","direction":"   "}"#).unwrap();
        assert_eq!(intent, UserInputIntent::Explore { direction: None });
    }
}
