use scraper::{ElementRef, Html, Node, Selector};
use url::Url;

use super::validate_url;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedPage {
    pub title: String,
    pub canonical_url: String,
    pub text: String,
}

pub(crate) fn extract_page(final_url: &Url, media_type: &str, source: &str) -> ExtractedPage {
    if media_type == "text/plain" {
        return ExtractedPage {
            title: String::new(),
            canonical_url: final_url.as_str().to_owned(),
            text: normalize_text(source),
        };
    }

    let document = Html::parse_document(source);
    let title_selector = Selector::parse("title").expect("static title selector must be valid");
    let body_selector = Selector::parse("body").expect("static body selector must be valid");
    let canonical_selector =
        Selector::parse("link[rel][href]").expect("static canonical selector must be valid");
    let title = document
        .select(&title_selector)
        .next()
        .map(|element| normalize_text(&element.text().collect::<Vec<_>>().join(" ")))
        .unwrap_or_default();
    let canonical_url = document
        .select(&canonical_selector)
        .find(|element| {
            element.value().attr("rel").is_some_and(|value| {
                value
                    .split_ascii_whitespace()
                    .any(|relation| relation.eq_ignore_ascii_case("canonical"))
            })
        })
        .and_then(|element| element.value().attr("href"))
        .and_then(|href| final_url.join(href).ok())
        .and_then(|url| validate_url(url.as_str()).ok())
        .map(|url| url.as_str().to_owned())
        .unwrap_or_else(|| final_url.as_str().to_owned());
    let text = document
        .select(&body_selector)
        .next()
        .map(visible_text)
        .unwrap_or_default();

    ExtractedPage {
        title,
        canonical_url,
        text,
    }
}

fn visible_text(body: ElementRef<'_>) -> String {
    let mut visible = String::new();
    for node in body.descendants() {
        let Node::Text(text) = node.value() else {
            continue;
        };
        if node.ancestors().filter_map(ElementRef::wrap).any(is_hidden) {
            continue;
        }
        visible.push_str(text);
        visible.push(' ');
    }
    normalize_text(&visible)
}

fn is_hidden(element: ElementRef<'_>) -> bool {
    if matches!(
        element.value().name(),
        "script"
            | "style"
            | "nav"
            | "form"
            | "noscript"
            | "template"
            | "iframe"
            | "object"
            | "embed"
            | "canvas"
            | "svg"
            | "audio"
            | "video"
    ) || element.value().attr("hidden").is_some()
        || element.value().attr("inert").is_some()
        || element
            .value()
            .attr("aria-hidden")
            .is_some_and(|value| value.eq_ignore_ascii_case("true"))
    {
        return true;
    }

    element.value().attr("style").is_some_and(|style| {
        style.split(';').any(|declaration| {
            declaration
                .split_once(':')
                .is_some_and(|(property, value)| {
                    let property = property.trim();
                    (property.eq_ignore_ascii_case("display") && css_value_eq(value, "none"))
                        || (property.eq_ignore_ascii_case("visibility")
                            && (css_value_eq(value, "hidden") || css_value_eq(value, "collapse")))
                })
        })
    })
}

fn css_value_eq(value: &str, expected: &str) -> bool {
    let value = value.trim();
    let suffix_len = "!important".len();
    let value = value
        .len()
        .checked_sub(suffix_len)
        .and_then(|suffix_start| {
            value
                .get(suffix_start..)
                .filter(|suffix| suffix.eq_ignore_ascii_case("!important"))
                .and_then(|_| value.get(..suffix_start))
        })
        .unwrap_or(value)
        .trim_end();
    value.eq_ignore_ascii_case(expected)
}

pub(crate) fn normalize_text(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(12_000)
        .collect()
}
