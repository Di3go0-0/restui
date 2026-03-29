use anyhow::Result;

use crate::model::request::{Header, HttpMethod, QueryParam, Request};

pub fn parse(input: &str) -> Result<Vec<Request>> {
    let blocks = split_request_blocks(input);
    let mut requests = Vec::new();

    for (line_offset, block) in blocks {
        if let Some(request) = parse_block(block, line_offset) {
            requests.push(request);
        }
    }

    Ok(requests)
}

fn split_request_blocks(input: &str) -> Vec<(usize, &str)> {
    let mut blocks = Vec::new();
    let mut current_start = 0;
    let mut block_start_line = 0;
    let mut in_block = false;

    for (i, line) in input.lines().enumerate() {
        if line.trim().starts_with("###") {
            if in_block {
                let block_end = input[current_start..]
                    .find("###")
                    .map(|pos| current_start + pos)
                    .unwrap_or(input.len());
                let block_text = &input[current_start..block_end];
                if !block_text.trim().is_empty() {
                    blocks.push((block_start_line, block_text));
                }
            }
            in_block = true;
            block_start_line = i + 1;
            // Advance past the ### line
            current_start = byte_offset_of_line(input, i + 1);
        }
    }

    // Last block
    if in_block {
        let remaining = &input[current_start..];
        if !remaining.trim().is_empty() {
            blocks.push((block_start_line, remaining));
        }
    } else if !input.trim().is_empty() {
        // No ### separators — entire input is one request
        blocks.push((0, input));
    }

    blocks
}

fn byte_offset_of_line(input: &str, line_num: usize) -> usize {
    let mut offset = 0;
    for (i, line) in input.lines().enumerate() {
        if i == line_num {
            return offset;
        }
        offset += line.len() + 1; // +1 for \n
    }
    input.len()
}

fn parse_block(block: &str, line_offset: usize) -> Option<Request> {
    let mut lines = block.lines().peekable();
    let mut name = None;

    // Skip comments and metadata, extract @name
    while let Some(line) = lines.peek() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            lines.next();
            continue;
        }
        if trimmed.starts_with('#') || trimmed.starts_with("//") {
            // Check for @name directive
            if let Some(rest) = trimmed
                .strip_prefix("# @name")
                .or_else(|| trimmed.strip_prefix("// @name"))
            {
                name = Some(rest.trim().to_string());
            }
            lines.next();
            continue;
        }
        break;
    }

    // Parse request line: METHOD URL [HTTP/version]
    let request_line = lines.next()?.trim();
    let (method, url) = parse_request_line(request_line)?;

    // Parse URL query params
    let (clean_url, query_params) = extract_query_params(&url);

    // Parse headers until blank line
    let mut headers = Vec::new();
    while let Some(line) = lines.peek() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            lines.next();
            break;
        }
        if let Some((key, value)) = trimmed.split_once(':') {
            headers.push(Header {
                name: key.trim().to_string(),
                value: value.trim().to_string(),
                enabled: true,
            });
        }
        lines.next();
    }

    // Remaining lines are body
    let body_lines: Vec<&str> = lines.collect();
    let body = if body_lines.is_empty() {
        None
    } else {
        let body_text = body_lines.join("\n");
        if body_text.trim().is_empty() {
            None
        } else {
            Some(body_text)
        }
    };

    Some(Request {
        name,
        method,
        url: clean_url,
        headers,
        query_params,
        cookies: Vec::new(),
        path_params: Vec::new(),
        body,
        source_file: None,
        source_line: Some(line_offset),
    })
}

fn parse_request_line(line: &str) -> Option<(HttpMethod, String)> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 2 {
        return None;
    }

    let method = HttpMethod::from_str(parts[0])?;
    let url = parts[1].to_string();

    Some((method, url))
}

fn extract_query_params(url: &str) -> (String, Vec<QueryParam>) {
    if let Some((base, query)) = url.split_once('?') {
        let params: Vec<QueryParam> = query
            .split('&')
            .filter_map(|pair| {
                let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
                if key.is_empty() {
                    None
                } else {
                    Some(QueryParam {
                        key: key.to_string(),
                        value: value.to_string(),
                        enabled: true,
                    })
                }
            })
            .collect();
        (base.to_string(), params)
    } else {
        (url.to_string(), Vec::new())
    }
}

/// Serialize a list of requests back to .http format
pub fn serialize(requests: &[Request]) -> String {
    let mut output = String::new();

    for (i, req) in requests.iter().enumerate() {
        if i > 0 {
            output.push_str("\n");
        }

        // Separator + name
        if let Some(ref name) = req.name {
            output.push_str(&format!("### {}\n", name));
            output.push_str(&format!("# @name {}\n", name));
        } else {
            output.push_str(&format!("### {} {}\n", req.method, req.url));
        }

        // Request line with query params
        let url = if req.query_params.is_empty() {
            req.url.clone()
        } else {
            let qs: Vec<String> = req
                .query_params
                .iter()
                .filter(|p| p.enabled)
                .map(|p| format!("{}={}", p.key, p.value))
                .collect();
            format!("{}?{}", req.url, qs.join("&"))
        };
        output.push_str(&format!("{} {}\n", req.method, url));

        // Headers
        for header in &req.headers {
            if header.enabled {
                output.push_str(&format!("{}: {}\n", header.name, header.value));
            } else {
                // Disabled headers as comments
                output.push_str(&format!("# {}: {}\n", header.name, header.value));
            }
        }

        // Body
        if let Some(ref body) = req.body {
            let trimmed = body.trim();
            if !trimmed.is_empty() {
                output.push_str("\n");
                output.push_str(trimmed);
                output.push_str("\n");
            }
        }
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_get() {
        let input = "GET https://api.example.com/users";
        let requests = parse(input).unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method, HttpMethod::GET);
        assert_eq!(requests[0].url, "https://api.example.com/users");
    }

    #[test]
    fn test_parse_with_headers() {
        let input = r#"POST https://api.example.com/users
Content-Type: application/json
Authorization: Bearer token123

{"name": "Diego"}"#;

        let requests = parse(input).unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method, HttpMethod::POST);
        assert_eq!(requests[0].headers.len(), 2);
        assert_eq!(requests[0].headers[0].name, "Content-Type");
        assert!(requests[0].body.is_some());
    }

    #[test]
    fn test_parse_multiple_requests() {
        let input = r#"### Get Users
GET https://api.example.com/users

### Create User
POST https://api.example.com/users
Content-Type: application/json

{"name": "Diego"}"#;

        let requests = parse(input).unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].method, HttpMethod::GET);
        assert_eq!(requests[1].method, HttpMethod::POST);
    }

    #[test]
    fn test_parse_with_name() {
        let input = r#"# @name GetUsers
GET https://api.example.com/users"#;

        let requests = parse(input).unwrap();
        assert_eq!(requests[0].name, Some("GetUsers".to_string()));
    }

    #[test]
    fn test_parse_query_params() {
        let input = "GET https://api.example.com/users?page=1&limit=10";
        let requests = parse(input).unwrap();
        assert_eq!(requests[0].query_params.len(), 2);
        assert_eq!(requests[0].query_params[0].key, "page");
        assert_eq!(requests[0].query_params[0].value, "1");
    }
}
