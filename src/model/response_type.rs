use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum JsonType {
    Null,
    Bool,
    Number,
    String,
    Array(Box<JsonType>),
    Object(Vec<(std::string::String, JsonType)>),
}

impl JsonType {
    /// Infer a JsonType from a serde_json::Value
    pub fn infer(value: &serde_json::Value) -> Self {
        match value {
            serde_json::Value::Null => JsonType::Null,
            serde_json::Value::Bool(_) => JsonType::Bool,
            serde_json::Value::Number(_) => JsonType::Number,
            serde_json::Value::String(_) => JsonType::String,
            serde_json::Value::Array(arr) => {
                if arr.is_empty() {
                    JsonType::Array(Box::new(JsonType::Null))
                } else {
                    // Infer from first element
                    JsonType::Array(Box::new(JsonType::infer(&arr[0])))
                }
            }
            serde_json::Value::Object(map) => {
                let fields: Vec<(std::string::String, JsonType)> = map.iter()
                    .map(|(k, v)| (k.clone(), JsonType::infer(v)))
                    .collect();
                JsonType::Object(fields)
            }
        }
    }

    /// Get display label for this type
    pub fn label(&self) -> &str {
        match self {
            JsonType::Null => "null",
            JsonType::Bool => "boolean",
            JsonType::Number => "number",
            JsonType::String => "string",
            JsonType::Array(_) => "array",
            JsonType::Object(_) => "object",
        }
    }

    /// Convert to TypeScript-like display lines
    pub fn to_display_lines(&self, indent: usize) -> Vec<std::string::String> {
        let pad = "  ".repeat(indent);
        match self {
            JsonType::Null => vec![format!("{}null", pad)],
            JsonType::Bool => vec![format!("{}boolean", pad)],
            JsonType::Number => vec![format!("{}number", pad)],
            JsonType::String => vec![format!("{}string", pad)],
            JsonType::Array(inner) => {
                match inner.as_ref() {
                    JsonType::Object(fields) => {
                        let mut lines = vec![format!("{}{{", pad)];
                        for (i, (key, val)) in fields.iter().enumerate() {
                            let comma = if i < fields.len() - 1 { "," } else { "" };
                            match val {
                                JsonType::Object(_) | JsonType::Array(_) => {
                                    lines.push(format!("{}  {}: ", pad, key));
                                    let sub = val.to_display_lines(indent + 2);
                                    lines.extend(sub);
                                    // Add comma to last line
                                    if let Some(last) = lines.last_mut() {
                                        last.push_str(comma);
                                    }
                                }
                                _ => {
                                    lines.push(format!("{}  {}: {}{}", pad, key, val.label(), comma));
                                }
                            }
                        }
                        lines.push(format!("{}}}[]", pad));
                        lines
                    }
                    _ => vec![format!("{}{}[]", pad, inner.label())],
                }
            }
            JsonType::Object(fields) => {
                let mut lines = vec![format!("{}{{", pad)];
                for (i, (key, val)) in fields.iter().enumerate() {
                    let comma = if i < fields.len() - 1 { "," } else { "" };
                    match val {
                        JsonType::Object(_) | JsonType::Array(_) => {
                            lines.push(format!("{}  {}: ", pad, key));
                            let sub = val.to_display_lines(indent + 1);
                            lines.extend(sub);
                            if let Some(last) = lines.last_mut() {
                                last.push_str(comma);
                            }
                        }
                        _ => {
                            lines.push(format!("{}  {}: {}{}", pad, key, val.label(), comma));
                        }
                    }
                }
                lines.push(format!("{}}}", pad));
                lines
            }
        }
    }

    /// Get field names at the top level (for autocomplete)
    #[allow(dead_code)]
    pub fn field_names(&self) -> Vec<std::string::String> {
        match self {
            JsonType::Object(fields) => fields.iter().map(|(k, _)| k.clone()).collect(),
            JsonType::Array(inner) => {
                // Return inner object fields with [n] prefix hint
                inner.field_names()
            }
            _ => vec![],
        }
    }

    /// Navigate to a subtype by field name
    #[allow(dead_code)]
    pub fn get_field(&self, name: &str) -> Option<&JsonType> {
        match self {
            JsonType::Object(fields) => fields.iter().find(|(k, _)| k == name).map(|(_, v)| v),
            JsonType::Array(inner) => inner.get_field(name),
            _ => None,
        }
    }

    /// Validate a JSON value against this type, returning mismatches
    #[allow(dead_code)]
    pub fn validate(&self, value: &serde_json::Value) -> Vec<TypeMismatch> {
        let mut mismatches = Vec::new();
        self.validate_inner(value, "", &mut mismatches);
        mismatches
    }

    fn validate_inner(&self, value: &serde_json::Value, path: &str, mismatches: &mut Vec<TypeMismatch>) {
        match (self, value) {
            (JsonType::Null, serde_json::Value::Null) => {}
            (JsonType::Bool, serde_json::Value::Bool(_)) => {}
            (JsonType::Number, serde_json::Value::Number(_)) => {}
            (JsonType::String, serde_json::Value::String(_)) => {}
            (JsonType::Array(expected_inner), serde_json::Value::Array(arr)) => {
                if let Some(first) = arr.first() {
                    let child_path = format!("{}[0]", path);
                    expected_inner.validate_inner(first, &child_path, mismatches);
                }
            }
            (JsonType::Object(expected_fields), serde_json::Value::Object(map)) => {
                for (key, expected_type) in expected_fields {
                    let child_path = if path.is_empty() { key.clone() } else { format!("{}.{}", path, key) };
                    if let Some(actual_value) = map.get(key) {
                        expected_type.validate_inner(actual_value, &child_path, mismatches);
                    } else {
                        mismatches.push(TypeMismatch {
                            path: child_path,
                            expected: expected_type.label().to_string(),
                            actual: "missing".to_string(),
                        });
                    }
                }
                // Check for extra fields not in expected type
                for key in map.keys() {
                    if !expected_fields.iter().any(|(k, _)| k == key) {
                        let child_path = if path.is_empty() { key.clone() } else { format!("{}.{}", path, key) };
                        mismatches.push(TypeMismatch {
                            path: child_path,
                            expected: "not expected".to_string(),
                            actual: JsonType::infer(&map[key]).label().to_string(),
                        });
                    }
                }
            }
            _ => {
                mismatches.push(TypeMismatch {
                    path: path.to_string(),
                    expected: self.label().to_string(),
                    actual: JsonType::infer(value).label().to_string(),
                });
            }
        }
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct TypeMismatch {
    pub path: String,
    pub expected: String,
    pub actual: String,
}
