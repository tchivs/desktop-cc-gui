use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde_json::{json, Value};
use std::path::PathBuf;

use super::SendMessageParams;

/// Format the user's AskUserQuestion answers into a human-readable message
/// that can be sent as a follow-up via `--resume`.
pub(super) fn format_ask_user_answer(result: &Value) -> String {
    let mut parts = Vec::new();

    if let Some(answers_obj) = result.get("answers").and_then(|a| a.as_object()) {
        for (_key, entry) in answers_obj {
            if let Some(arr) = entry.get("answers").and_then(|a| a.as_array()) {
                let texts: Vec<&str> = arr.iter().filter_map(|v| v.as_str()).collect();
                if !texts.is_empty() {
                    parts.push(texts.join(", "));
                }
            }
        }
    }

    if parts.is_empty() {
        "The user dismissed the question without selecting an option.".to_string()
    } else {
        format!(
            "The user answered the AskUserQuestion: {}. Please continue based on this selection.",
            parts.join("; ")
        )
    }
}

/// Build message content with images for stream-json input
pub(super) fn build_message_content(params: &SendMessageParams) -> Result<Value, String> {
    let mut content = Vec::new();
    let mut image_failures: Vec<String> = Vec::new();

    if let Some(ref images) = params.images {
        for image_path in images {
            let trimmed = image_path.trim();
            if trimmed.is_empty() {
                continue;
            }

            if trimmed.starts_with("data:") {
                let parts: Vec<&str> = trimmed.splitn(2, ',').collect();
                if parts.len() == 2 {
                    // Some callers may accidentally pass `data:image/...;base64,file:///...`.
                    // Recover by treating this payload as a file URI path.
                    if parts[1].starts_with("file://") {
                        push_file_image_content(parts[1], &mut content, &mut image_failures);
                        continue;
                    }
                    let media_type = parts[0]
                        .strip_prefix("data:")
                        .and_then(|s| s.strip_suffix(";base64"))
                        .unwrap_or("image/png");
                    let normalized_base64 = normalize_base64_payload(parts[1]);
                    if normalized_base64.is_empty() {
                        image_failures.push("empty data-url base64 payload".to_string());
                        continue;
                    }
                    if STANDARD.decode(normalized_base64.as_bytes()).is_err() {
                        image_failures
                            .push("invalid data-url base64 payload (decode failed)".to_string());
                        continue;
                    }
                    content.push(json!({
                        "type": "image",
                        "source": {
                            "type": "base64",
                            "media_type": media_type,
                            "data": normalized_base64
                        }
                    }));
                }
            } else if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
                content.push(json!({
                    "type": "image",
                    "source": {
                        "type": "url",
                        "url": trimmed
                    }
                }));
            } else {
                push_file_image_content(trimmed, &mut content, &mut image_failures);
            }
        }
    }
    if !image_failures.is_empty() {
        return Err(format!(
            "Failed to attach image inputs for Claude: {}",
            image_failures.join("; ")
        ));
    }

    if !params.text.trim().is_empty() {
        content.push(json!({
            "type": "text",
            "text": params.text.trim()
        }));
    }

    Ok(json!({
        "type": "user",
        "message": {
            "role": "user",
            "content": content
        }
    }))
}

fn push_file_image_content(input: &str, content: &mut Vec<Value>, failures: &mut Vec<String>) {
    let path = normalize_image_path(input);
    match std::fs::read(&path) {
        Ok(data) => {
            let base64_data = STANDARD.encode(&data);
            let media_type = match path
                .extension()
                .and_then(|e| e.to_str())
                .map(|ext| ext.to_ascii_lowercase())
                .as_deref()
            {
                Some("png") => "image/png",
                Some("jpg") | Some("jpeg") => "image/jpeg",
                Some("gif") => "image/gif",
                Some("webp") => "image/webp",
                Some("bmp") => "image/bmp",
                Some("svg") => "image/svg+xml",
                Some("tif") | Some("tiff") => "image/tiff",
                _ => "image/png",
            };
            content.push(json!({
                "type": "image",
                "source": {
                    "type": "base64",
                    "media_type": media_type,
                    "data": base64_data
                }
            }));
        }
        Err(error) => {
            failures.push(format!("{} ({})", path.display(), error));
        }
    }
}

fn normalize_image_path(input: &str) -> PathBuf {
    if let Some(path) = parse_file_uri_path(input) {
        return PathBuf::from(path);
    }
    PathBuf::from(input)
}

fn parse_file_uri_path(raw_uri: &str) -> Option<String> {
    let without_scheme = raw_uri.strip_prefix("file://")?;
    let (host, path_part) = if without_scheme.starts_with('/') {
        ("", without_scheme.to_string())
    } else if let Some((host, rest)) = without_scheme.split_once('/') {
        (host, format!("/{}", rest))
    } else {
        (without_scheme, "/".to_string())
    };

    let decoded_path = percent_decode_path(&path_part);
    let is_local_host = host.is_empty() || host.eq_ignore_ascii_case("localhost");
    let mut normalized = if is_local_host {
        decoded_path
    } else {
        format!("//{}{}", host, decoded_path)
    };

    if cfg!(windows)
        && is_local_host
        && normalized.starts_with('/')
        && has_windows_drive_prefix(&normalized[1..])
    {
        normalized = normalized[1..].to_string();
    }

    Some(normalized)
}

fn has_windows_drive_prefix(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'/' || bytes[2] == b'\\')
}

fn percent_decode_path(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut idx = 0usize;
    while idx < bytes.len() {
        if bytes[idx] == b'%' && idx + 2 < bytes.len() {
            let h1 = bytes[idx + 1];
            let h2 = bytes[idx + 2];
            let v1 = hex_value(h1);
            let v2 = hex_value(h2);
            if let (Some(a), Some(b)) = (v1, v2) {
                out.push((a << 4) | b);
                idx += 3;
                continue;
            }
        }
        out.push(bytes[idx]);
        idx += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn normalize_base64_payload(input: &str) -> String {
    input.chars().filter(|c| !c.is_ascii_whitespace()).collect()
}

#[cfg(test)]
mod tests {
    use super::{build_message_content, normalize_image_path};
    use crate::engine::SendMessageParams;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_file_path(name: &str) -> std::path::PathBuf {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        std::env::temp_dir().join(format!("moss-x-claude-image-{}-{}", timestamp, name))
    }

    #[test]
    fn build_message_content_supports_file_uri_images() {
        let image_path = temp_file_path("file-uri.png");
        std::fs::write(&image_path, [0x89, b'P', b'N', b'G']).expect("write");
        let image_uri = format!("file://{}", image_path.to_string_lossy());

        let mut params = SendMessageParams::default();
        params.text = "describe".to_string();
        params.images = Some(vec![image_uri]);

        let content = build_message_content(&params).expect("content");
        let blocks = content["message"]["content"].as_array().expect("array");
        assert_eq!(blocks[0]["type"], "image");
        assert_eq!(blocks[0]["source"]["type"], "base64");
        assert_eq!(blocks[0]["source"]["media_type"], "image/png");
        assert_eq!(blocks[1]["type"], "text");

        let _ = std::fs::remove_file(image_path);
    }

    #[test]
    fn build_message_content_recovers_miswrapped_data_url_file_uri() {
        let image_path = temp_file_path("miswrapped.jpg");
        std::fs::write(&image_path, [0xFF, 0xD8, 0xFF]).expect("write");
        let image_uri = format!("file://{}", image_path.to_string_lossy());

        let mut params = SendMessageParams::default();
        params.text = "describe".to_string();
        params.images = Some(vec![format!("data:image/jpeg;base64,{}", image_uri)]);

        let content = build_message_content(&params).expect("content");
        let blocks = content["message"]["content"].as_array().expect("array");
        assert_eq!(blocks[0]["type"], "image");
        assert_eq!(blocks[0]["source"]["type"], "base64");
        assert_eq!(blocks[0]["source"]["media_type"], "image/jpeg");

        let _ = std::fs::remove_file(image_path);
    }

    #[test]
    fn build_message_content_returns_error_when_image_path_unreadable() {
        let mut params = SendMessageParams::default();
        params.text = "describe".to_string();
        params.images = Some(vec!["/tmp/does-not-exist-moss-x-image.png".to_string()]);

        let error = build_message_content(&params).expect_err("expected error");
        assert!(error.contains("Failed to attach image inputs for Claude"));
    }

    #[test]
    fn build_message_content_returns_error_when_data_url_base64_invalid() {
        let mut params = SendMessageParams::default();
        params.text = "describe".to_string();
        params.images = Some(vec![
            "data:image/png;base64,not-valid-base64-***".to_string()
        ]);

        let error = build_message_content(&params).expect_err("expected error");
        assert!(error.contains("Failed to attach image inputs for Claude"));
        assert!(error.contains("invalid data-url base64 payload"));
    }

    #[test]
    fn normalize_image_path_decodes_localhost_file_uri() {
        let normalized = normalize_image_path("file://localhost/tmp/a%20b.png");
        assert_eq!(normalized.to_string_lossy(), "/tmp/a b.png");
    }

    #[test]
    fn normalize_image_path_preserves_unc_host() {
        let normalized = normalize_image_path("file://server/share/folder/a%20b.png");
        assert_eq!(
            normalized.to_string_lossy(),
            "//server/share/folder/a b.png"
        );
    }

    #[test]
    fn normalize_image_path_handles_windows_drive_uri_prefix() {
        let normalized = normalize_image_path("file:///C:/Users/demo/a%20b.png");
        let expected = if cfg!(windows) {
            "C:/Users/demo/a b.png"
        } else {
            "/C:/Users/demo/a b.png"
        };
        assert_eq!(normalized.to_string_lossy(), expected);
    }
}
