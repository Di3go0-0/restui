/// Request chaining: resolve `{{@request_name.json.path}}` references
/// by extracting values from other requests' cached responses.

/// A parsed chain reference extracted from `{{@...}}` syntax.
#[derive(Debug, Clone, PartialEq)]
pub struct ChainRef {
    /// Optional collection name (before `/`). None = search all collections.
    pub collection: Option<String>,
    /// The request name to look up.
    pub request_name: String,
    /// Dot-separated JSON path with optional array indices, e.g. `data[0].token`.
    pub json_path: String,
}

/// Errors during chain resolution.
#[derive(Debug)]
pub enum ChainError {
    RequestNotFound { name: String },
    CircularDependency { chain: Vec<String> },
    JsonPathNotFound { path: String },
    ResponseNotJson { request_name: String },
    DependencyFailed { request_name: String, error: String },
}

impl std::fmt::Display for ChainError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChainError::RequestNotFound { name } => {
                write!(f, "Chain error: request '{}' not found in any collection", name)
            }
            ChainError::CircularDependency { chain } => {
                write!(f, "Chain error: circular dependency: {}", chain.join(" → "))
            }
            ChainError::JsonPathNotFound { path } => {
                write!(f, "Chain error: path '{}' not found in response", path)
            }
            ChainError::ResponseNotJson { request_name } => {
                write!(f, "Chain error: response from '{}' is not valid JSON", request_name)
            }
            ChainError::DependencyFailed { request_name, error } => {
                write!(f, "Chain error: dependency '{}' failed: {}", request_name, error)
            }
        }
    }
}

/// Parse the content between `{{@` and `}}`.
///
/// Format: `[collection/]request_name.json.path`
///
/// Examples:
/// - `login.token` → ChainRef { collection: None, request_name: "login", json_path: "token" }
/// - `auth/login.data[0].token` → ChainRef { collection: Some("auth"), request_name: "login", json_path: "data[0].token" }
pub fn parse_chain_ref(raw: &str) -> Option<ChainRef> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }

    // Split collection prefix if present: "collection/rest" or just "rest"
    let (collection, remainder) = if let Some(slash_pos) = raw.find('/') {
        let coll = &raw[..slash_pos];
        let rest = &raw[slash_pos + 1..];
        if coll.is_empty() || rest.is_empty() {
            return None;
        }
        (Some(coll.to_string()), rest)
    } else {
        (None, raw)
    };

    // Split request_name from json_path at first '.'
    let (request_name, json_path) = if let Some(dot_pos) = remainder.find('.') {
        let name = &remainder[..dot_pos];
        let path = &remainder[dot_pos + 1..];
        if name.is_empty() || path.is_empty() {
            return None;
        }
        (name.to_string(), path.to_string())
    } else {
        return None; // Must have at least request_name.field
    };

    Some(ChainRef {
        collection,
        request_name,
        json_path,
    })
}

/// Find all `{{@...}}` references in a template string.
/// Returns (start_byte, end_byte, inner_content) for each match.
/// end_byte is exclusive (points past the closing `}}`).
pub fn find_chain_refs(template: &str) -> Vec<(usize, usize, String)> {
    let mut refs = Vec::new();
    let bytes = template.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i + 4 < len {
        // Look for {{@
        if bytes[i] == b'{' && bytes[i + 1] == b'{' && bytes[i + 2] == b'@' {
            let start = i;
            let content_start = i + 3; // after {{@
            // Find closing }}
            if let Some(close_pos) = template[content_start..].find("}}") {
                let content_end = content_start + close_pos;
                let end = content_end + 2; // past }}
                let inner = &template[content_start..content_end];
                if !inner.is_empty() {
                    refs.push((start, end, inner.to_string()));
                }
                i = end;
            } else {
                i += 1;
            }
        } else {
            i += 1;
        }
    }

    refs
}

/// Extract a value from a JSON string using a dot-path with optional array indices.
///
/// Path segments: `field`, `field[0]`, `nested.field`, `data[0].items[1].name`
///
/// Returns the extracted value as a string:
/// - Strings: returned without quotes
/// - Numbers/bools/null: returned as-is
/// - Objects/arrays: returned as compact JSON
pub fn extract_json_value(json_str: &str, path: &str) -> Result<String, ChainError> {
    // Trim BOM and whitespace before parsing
    let trimmed = json_str.trim().trim_start_matches('\u{feff}');
    let root: serde_json::Value = serde_json::from_str(trimmed).map_err(|_| {
        ChainError::ResponseNotJson {
            request_name: path.to_string(),
        }
    })?;

    let mut current = &root;

    for segment in split_path_segments(path) {
        // Check for array index: "field[N]"
        if let Some((field, index)) = parse_array_segment(&segment) {
            // Navigate to field first (if not empty, e.g., bare "[0]")
            if !field.is_empty() {
                current = current.get(&field).ok_or_else(|| ChainError::JsonPathNotFound {
                    path: path.to_string(),
                })?;
            }
            // Then index into array
            current = current.get(index).ok_or_else(|| ChainError::JsonPathNotFound {
                path: path.to_string(),
            })?;
        } else {
            // Plain field access
            current = current.get(segment.as_str()).ok_or_else(|| ChainError::JsonPathNotFound {
                path: path.to_string(),
            })?;
        }
    }

    // Convert to string
    match current {
        serde_json::Value::String(s) => Ok(s.clone()),
        serde_json::Value::Null => Ok("null".to_string()),
        serde_json::Value::Bool(b) => Ok(b.to_string()),
        serde_json::Value::Number(n) => Ok(n.to_string()),
        other => Ok(other.to_string()), // compact JSON for objects/arrays
    }
}

/// Split a dot-path into segments, respecting brackets.
/// "data[0].items[1].name" → ["data[0]", "items[1]", "name"]
fn split_path_segments(path: &str) -> Vec<String> {
    let mut segments = Vec::new();
    let mut current = String::new();

    for ch in path.chars() {
        if ch == '.' && !current.is_empty() {
            segments.push(std::mem::take(&mut current));
        } else {
            current.push(ch);
        }
    }
    if !current.is_empty() {
        segments.push(current);
    }

    segments
}

/// Parse "field[N]" into (field_name, index). Returns None if no bracket notation.
fn parse_array_segment(segment: &str) -> Option<(String, usize)> {
    let bracket_start = segment.find('[')?;
    let bracket_end = segment.find(']')?;
    if bracket_end <= bracket_start + 1 {
        return None;
    }
    let field = segment[..bracket_start].to_string();
    let index_str = &segment[bracket_start + 1..bracket_end];
    let index = index_str.parse::<usize>().ok()?;
    Some((field, index))
}

#[cfg(test)]
mod tests {
    use super::*;

    // === parse_chain_ref tests ===

    #[test]
    fn test_parse_simple_ref() {
        let r = parse_chain_ref("login.token").unwrap();
        assert_eq!(r.collection, None);
        assert_eq!(r.request_name, "login");
        assert_eq!(r.json_path, "token");
    }

    #[test]
    fn test_parse_with_collection() {
        let r = parse_chain_ref("auth/login.data[0].token").unwrap();
        assert_eq!(r.collection, Some("auth".to_string()));
        assert_eq!(r.request_name, "login");
        assert_eq!(r.json_path, "data[0].token");
    }

    #[test]
    fn test_parse_nested_path() {
        let r = parse_chain_ref("login.response.items[0].id").unwrap();
        assert_eq!(r.request_name, "login");
        assert_eq!(r.json_path, "response.items[0].id");
    }

    #[test]
    fn test_parse_empty_returns_none() {
        assert!(parse_chain_ref("").is_none());
    }

    #[test]
    fn test_parse_no_dot_returns_none() {
        assert!(parse_chain_ref("login").is_none());
    }

    #[test]
    fn test_parse_empty_path_returns_none() {
        assert!(parse_chain_ref("login.").is_none());
    }

    #[test]
    fn test_parse_empty_name_returns_none() {
        assert!(parse_chain_ref(".token").is_none());
    }

    // === find_chain_refs tests ===

    #[test]
    fn test_find_single_ref() {
        let refs = find_chain_refs("Bearer {{@login.token}}");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].2, "login.token");
        assert_eq!(&"Bearer {{@login.token}}"[refs[0].0..refs[0].1], "{{@login.token}}");
    }

    #[test]
    fn test_find_multiple_refs() {
        let refs = find_chain_refs("{{@a.x}} and {{@b.y}}");
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].2, "a.x");
        assert_eq!(refs[1].2, "b.y");
    }

    #[test]
    fn test_find_no_refs() {
        let refs = find_chain_refs("no refs here {{env_var}}");
        assert_eq!(refs.len(), 0);
    }

    #[test]
    fn test_find_mixed_with_env_vars() {
        let refs = find_chain_refs("{{base_url}}/api?token={{@login.token}}");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].2, "login.token");
    }

    // === extract_json_value tests ===

    #[test]
    fn test_extract_top_level_string() {
        let json = r#"{"token":"abc123"}"#;
        assert_eq!(extract_json_value(json, "token").unwrap(), "abc123");
    }

    #[test]
    fn test_extract_number() {
        let json = r#"{"count": 42}"#;
        assert_eq!(extract_json_value(json, "count").unwrap(), "42");
    }

    #[test]
    fn test_extract_boolean() {
        let json = r#"{"active": true}"#;
        assert_eq!(extract_json_value(json, "active").unwrap(), "true");
    }

    #[test]
    fn test_extract_nested() {
        let json = r#"{"data":{"user":{"name":"Juan"}}}"#;
        assert_eq!(extract_json_value(json, "data.user.name").unwrap(), "Juan");
    }

    #[test]
    fn test_extract_array_index() {
        let json = r#"{"items":[{"id":1},{"id":2}]}"#;
        assert_eq!(extract_json_value(json, "items[0].id").unwrap(), "1");
        assert_eq!(extract_json_value(json, "items[1].id").unwrap(), "2");
    }

    #[test]
    fn test_extract_nested_arrays() {
        let json = r#"{"data":[{"items":[{"name":"first"},{"name":"second"}]}]}"#;
        assert_eq!(
            extract_json_value(json, "data[0].items[1].name").unwrap(),
            "second"
        );
    }

    #[test]
    fn test_extract_not_found() {
        let json = r#"{"token":"abc"}"#;
        assert!(extract_json_value(json, "missing").is_err());
    }

    #[test]
    fn test_extract_array_out_of_bounds() {
        let json = r#"{"items":[{"id":1}]}"#;
        assert!(extract_json_value(json, "items[5].id").is_err());
    }

    #[test]
    fn test_extract_invalid_json() {
        assert!(extract_json_value("not json", "field").is_err());
    }

    #[test]
    fn test_extract_null() {
        let json = r#"{"value": null}"#;
        assert_eq!(extract_json_value(json, "value").unwrap(), "null");
    }

    #[test]
    fn test_extract_object_returns_compact_json() {
        let json = r#"{"nested": {"a": 1, "b": 2}}"#;
        let result = extract_json_value(json, "nested").unwrap();
        // Should return compact JSON
        assert!(result.contains("\"a\""));
        assert!(result.contains("\"b\""));
    }
}
