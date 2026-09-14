use crate::domain::ExplorationResult;
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContractViolation {
    InvalidEnvelope,
    ItemCount(usize),
}

pub fn parse_exploration_result(raw: &str) -> Result<ExplorationResult, ContractViolation> {
    let payload = extract_json_payload(raw).ok_or(ContractViolation::InvalidEnvelope)?;
    let envelope: ExplorationEnvelope =
        serde_json::from_str(payload).map_err(|_| ContractViolation::InvalidEnvelope)?;

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

    Ok(ExplorationResult {
        items: items
            .try_into()
            .map_err(|items: Vec<String>| ContractViolation::ItemCount(items.len()))?,
        diary: String::new(),
        sources: Vec::new(),
        images: Vec::new(),
        round_number: 0,
        elapsed_seconds: 0,
        raw_response: raw.to_string(),
        highlights: Vec::new(),
        sections: Vec::new(),
    })
}

pub fn build_contract_correction(raw: &str, violation: ContractViolation) -> String {
    let violation = match violation {
        ContractViolation::InvalidEnvelope => "上次响应不是可解析的约定 JSON 对象。",
        ContractViolation::ItemCount(_) => "探索结果数量不是 4。",
    };

    format!("上次响应：\n{raw}\n\n{violation}")
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExplorationEnvelope {
    #[serde(default)]
    items: Vec<String>,
    /// The model sometimes expresses the shared thread of the four findings
    /// as an extra `theme` field. It carries no routing meaning — accept and
    /// ignore it so a stylistic flourish does not fail the whole outing.
    #[serde(default)]
    theme: Option<String>,
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
    use crate::exploration::parse_outing_diary;

    #[test]
    fn diary_parser_requires_a_nonempty_json_diary() {
        assert_eq!(
            parse_outing_diary(r#"{"diary":"第二轮回来啦"}"#).unwrap().text,
            "第二轮回来啦"
        );
        assert!(parse_outing_diary(r#"{"diary":" "}"#).is_err());
    }

    #[test]
    fn first_stage_requires_only_four_findings_without_an_automatic_next_request() {
        let result = parse_exploration_result(r#"{"items":["甲","乙","丙","丁"]}"#).unwrap();

        assert_eq!(result.items, ["甲", "乙", "丙", "丁"]);
    }

    #[test]
    fn accepts_an_optional_theme_field_but_rejects_an_automatic_next_request_field() {
        let with_theme = parse_exploration_result(
            r#"{"items":["甲","乙","丙","丁"],"theme":"所有榜单都在假装描述世界"}"#,
        )
        .unwrap();
        assert_eq!(with_theme.items, ["甲", "乙", "丙", "丁"]);

        let raw = r#"{"items":["甲","乙","丙","丁"],"next_outing_request":"自动再出去玩"}"#;

        assert_eq!(
            parse_exploration_result(raw).unwrap_err(),
            ContractViolation::InvalidEnvelope
        );
    }

    #[test]
    fn accepts_exactly_four_trimmed_free_strings() {
        let raw = r#"{"items":[" 甲 ","乙","丙","丁"]}"#;

        let result = parse_exploration_result(raw).unwrap();

        assert_eq!(result.items, ["甲", "乙", "丙", "丁"]);
        assert_eq!(result.raw_response, raw);
    }

    #[test]
    fn accepts_one_markdown_fenced_json_object() {
        let raw = "  ```json\r\n{\"items\":[\"甲\",\"乙\",\"丙\",\"丁\"]}\r\n```  ";

        let result = parse_exploration_result(raw).unwrap();

        assert_eq!(result.items, ["甲", "乙", "丙", "丁"]);
        assert_eq!(result.raw_response, raw);
    }

    #[test]
    fn rejects_any_result_count_other_than_four() {
        for (raw, expected_count) in [
            (r#"{"items":[]}"#, 0),
            (r#"{"items":["甲","乙","丙"]}"#, 3),
            (r#"{"items":["甲","乙","丙","丁","戊"]}"#, 5),
        ] {
            assert_eq!(
                parse_exploration_result(raw).unwrap_err(),
                ContractViolation::ItemCount(expected_count)
            );
        }
    }

    #[test]
    fn rejects_an_empty_result_as_a_missing_free_text_result() {
        let raw = r#"{"items":["甲","  ","丙","丁"]}"#;

        assert_eq!(
            parse_exploration_result(raw).unwrap_err(),
            ContractViolation::ItemCount(3)
        );
    }

    #[test]
    fn distinguishes_invalid_json_or_fences_from_a_valid_zero_item_envelope() {
        for raw in [
            "not json",
            "```json\n{}\n```\n```json\n{}\n```",
            "before\n```json\n{}\n```",
        ] {
            assert_eq!(
                parse_exploration_result(raw).unwrap_err(),
                ContractViolation::InvalidEnvelope
            );
        }
    }

    #[test]
    fn correction_names_only_the_actual_single_violation() {
        let raw = "上一份原始响应";
        let count = build_contract_correction(raw, ContractViolation::ItemCount(3));
        assert_eq!(count, "上次响应：\n上一份原始响应\n\n探索结果数量不是 4。");

        let invalid = build_contract_correction(raw, ContractViolation::InvalidEnvelope);
        assert_eq!(
            invalid,
            "上次响应：\n上一份原始响应\n\n上次响应不是可解析的约定 JSON 对象。"
        );

        for correction in [&count, &invalid] {
            for forbidden in [
                "为什么选择",
                "来源链接",
                "固定段落",
                "四个主题类别",
                "下一站必须",
            ] {
                assert!(!correction.contains(forbidden));
            }
        }
    }
}
