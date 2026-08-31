use crate::domain::ExplorationResult;
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContractViolation {
    ItemCount(usize),
    EmptyRequest,
}

pub fn parse_exploration_result(raw: &str) -> Result<ExplorationResult, ContractViolation> {
    let payload = extract_json_payload(raw).ok_or(ContractViolation::ItemCount(0))?;
    let envelope: ExplorationEnvelope =
        serde_json::from_str(payload).map_err(|_| ContractViolation::ItemCount(0))?;

    if envelope.items.len() != 4 {
        return Err(ContractViolation::ItemCount(envelope.items.len()));
    }

    let items = envelope
        .items
        .into_iter()
        .map(|item| item.trim().to_string())
        .collect::<Vec<_>>();
    let non_empty_count = items.iter().filter(|item| !item.is_empty()).count();
    if non_empty_count != 4 {
        return Err(ContractViolation::ItemCount(non_empty_count));
    }

    let next_outing_request = envelope.next_outing_request.trim().to_string();
    if next_outing_request.is_empty() {
        return Err(ContractViolation::EmptyRequest);
    }

    Ok(ExplorationResult {
        items: items
            .try_into()
            .map_err(|items: Vec<String>| ContractViolation::ItemCount(items.len()))?,
        next_outing_request,
        raw_response: raw.to_string(),
    })
}

pub fn build_contract_correction(raw: &str, violation: ContractViolation) -> String {
    let violation = match violation {
        ContractViolation::ItemCount(_) => "探索结果数量不是 4。",
        ContractViolation::EmptyRequest => "想再次出去玩的请求为空。",
    };

    format!("上次响应：\n{raw}\n\n{violation}")
}

#[derive(Deserialize)]
struct ExplorationEnvelope {
    #[serde(default)]
    items: Vec<String>,
    #[serde(default)]
    next_outing_request: String,
}

fn extract_json_payload(raw: &str) -> Option<&str> {
    let trimmed = raw.trim();
    if !trimmed.starts_with("```") {
        return Some(trimmed);
    }

    let mut lines = trimmed.lines();
    let opening = lines.next()?.trim();
    if opening != "```" && !opening.eq_ignore_ascii_case("```json") {
        return None;
    }

    let remaining = lines.collect::<Vec<_>>();
    let (closing, body) = remaining.split_last()?;
    if closing.trim() != "```" || body.iter().any(|line| line.trim().starts_with("```")) {
        return None;
    }

    Some(trimmed[opening.len()..trimmed.len() - closing.len()].trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_exactly_four_trimmed_free_strings_and_one_request() {
        let raw =
            r#"{"items":[" 甲 ","乙","丙","丁"],"next_outing_request":" 我还想出去玩，可以吗？ "}"#;

        let result = parse_exploration_result(raw).unwrap();

        assert_eq!(result.items, ["甲", "乙", "丙", "丁"]);
        assert_eq!(result.next_outing_request, "我还想出去玩，可以吗？");
        assert_eq!(result.raw_response, raw);
    }

    #[test]
    fn accepts_one_markdown_fenced_json_object() {
        let raw = "  ```json\r\n{\"items\":[\"甲\",\"乙\",\"丙\",\"丁\"],\"next_outing_request\":\"再去玩？\"}\r\n```  ";

        let result = parse_exploration_result(raw).unwrap();

        assert_eq!(result.items, ["甲", "乙", "丙", "丁"]);
        assert_eq!(result.raw_response, raw);
    }

    #[test]
    fn rejects_any_result_count_other_than_four() {
        for (raw, expected_count) in [
            (
                r#"{"items":["甲","乙","丙"],"next_outing_request":"再去玩？"}"#,
                3,
            ),
            (
                r#"{"items":["甲","乙","丙","丁","戊"],"next_outing_request":"再去玩？"}"#,
                5,
            ),
        ] {
            assert_eq!(
                parse_exploration_result(raw).unwrap_err(),
                ContractViolation::ItemCount(expected_count)
            );
        }
    }

    #[test]
    fn rejects_an_empty_result_as_a_missing_free_text_result() {
        let raw = r#"{"items":["甲","  ","丙","丁"],"next_outing_request":"再去玩？"}"#;

        assert_eq!(
            parse_exploration_result(raw).unwrap_err(),
            ContractViolation::ItemCount(3)
        );
    }

    #[test]
    fn rejects_an_empty_final_request() {
        let raw = r#"{"items":["甲","乙","丙","丁"],"next_outing_request":" \n "}"#;

        assert_eq!(
            parse_exploration_result(raw).unwrap_err(),
            ContractViolation::EmptyRequest
        );
    }

    #[test]
    fn malformed_or_extra_fenced_content_does_not_bypass_the_contract() {
        for raw in [
            "not json",
            "```json\n{}\n```\n```json\n{}\n```",
            "before\n```json\n{}\n```",
        ] {
            assert_eq!(
                parse_exploration_result(raw).unwrap_err(),
                ContractViolation::ItemCount(0)
            );
        }
    }

    #[test]
    fn correction_names_only_the_actual_single_violation() {
        let raw = "上一份原始响应";
        let count = build_contract_correction(raw, ContractViolation::ItemCount(3));
        assert!(count.contains(raw));
        assert!(count.contains("数量不是 4"));
        assert!(!count.contains("请求为空"));
        for forbidden in [
            "为什么选择",
            "来源链接",
            "固定段落",
            "四个主题类别",
            "下一站必须",
        ] {
            assert!(!count.contains(forbidden));
        }

        let request = build_contract_correction(raw, ContractViolation::EmptyRequest);
        assert!(request.contains(raw));
        assert!(request.contains("请求为空"));
        assert!(!request.contains("数量不是 4"));
        for forbidden in [
            "为什么选择",
            "来源链接",
            "固定段落",
            "四个主题类别",
            "下一站必须",
        ] {
            assert!(!request.contains(forbidden));
        }
    }
}
