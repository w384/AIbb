use serde_json::Value;

use crate::error::{AppError, ErrorCode};

#[derive(Debug, Default)]
pub struct SseDecoder {
    buffer: Vec<u8>,
    done: bool,
}

impl SseDecoder {
    pub fn push(&mut self, chunk: &[u8]) -> Result<Vec<String>, AppError> {
        if self.done {
            return Ok(Vec::new());
        }

        self.buffer.extend_from_slice(chunk);
        let mut deltas = Vec::new();

        while let Some((event_end, separator_len)) = next_event_boundary(&self.buffer) {
            let event = self.buffer[..event_end].to_vec();
            self.buffer.drain(..event_end + separator_len);
            let event = std::str::from_utf8(&event).map_err(invalid_sse)?;
            let data = event
                .lines()
                .map(|line| line.strip_suffix('\r').unwrap_or(line))
                .filter_map(|line| {
                    line.strip_prefix("data:")
                        .map(|value| value.strip_prefix(' ').unwrap_or(value))
                })
                .collect::<Vec<_>>()
                .join("\n");

            if data.is_empty() {
                continue;
            }
            if data.trim() == "[DONE]" {
                self.done = true;
                self.buffer.clear();
                break;
            }

            let value: Value = serde_json::from_str(&data).map_err(invalid_sse)?;
            if let Some(content) = value
                .pointer("/choices/0/delta/content")
                .and_then(Value::as_str)
            {
                if !content.is_empty() {
                    deltas.push(content.to_owned());
                }
            }
        }

        Ok(deltas)
    }

    pub fn is_done(&self) -> bool {
        self.done
    }

    pub fn finish(&mut self) -> Result<Vec<String>, AppError> {
        if self.done {
            return Ok(Vec::new());
        }
        if self.buffer.is_empty() {
            return Err(invalid_sse("stream ended before [DONE]"));
        }

        if self.buffer.ends_with(b"\r\n") {
            self.buffer.extend_from_slice(b"\r\n");
        } else if self.buffer.ends_with(b"\n") {
            self.buffer.push(b'\n');
        } else {
            self.buffer.extend_from_slice(b"\n\n");
        }
        let deltas = self.push(&[])?;

        if self.done {
            Ok(deltas)
        } else {
            Err(invalid_sse("stream ended before [DONE]"))
        }
    }
}

pub fn parse_sse_text(input: &str) -> Result<Vec<String>, AppError> {
    let mut decoder = SseDecoder::default();
    let mut deltas = decoder.push(input.as_bytes())?;
    deltas.extend(decoder.finish()?);
    Ok(deltas)
}

fn next_event_boundary(buffer: &[u8]) -> Option<(usize, usize)> {
    let lf = buffer.windows(2).position(|window| window == b"\n\n");
    let crlf = buffer.windows(4).position(|window| window == b"\r\n\r\n");

    match (lf, crlf) {
        (Some(left), Some(right)) if left <= right => Some((left, 2)),
        (Some(_), Some(right)) => Some((right, 4)),
        (Some(index), None) => Some((index, 2)),
        (None, Some(index)) => Some((index, 4)),
        (None, None) => None,
    }
}

fn invalid_sse(error: impl std::fmt::Display) -> AppError {
    AppError::from_code(ErrorCode::InvalidResponse)
        .with_diagnostic(format!("invalid SSE payload: {error}"), None)
}
