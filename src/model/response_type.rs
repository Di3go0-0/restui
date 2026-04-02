use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum JsonType {
    Null,
    Bool,
    Number,
    String,
    Buffer,
    Array(Box<JsonType>),
    Object(Vec<(std::string::String, JsonType)>),
    Enum(Vec<std::string::String>),
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
            JsonType::Buffer => "Buffer",
            JsonType::Array(_) => "array",
            JsonType::Object(_) => "object",
            JsonType::Enum(_) => "enum",
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
            JsonType::Buffer => vec![format!("{}Buffer", pad)],
            JsonType::Enum(values) => {
                let enum_str = values.iter()
                    .map(|v| format!("\"{}\"", v))
                    .collect::<Vec<_>>()
                    .join(" | ");
                vec![format!("{}{}", pad, enum_str)]
            }
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
                        JsonType::Enum(values) => {
                            let enum_str = values.iter()
                                .map(|v| format!("\"{}\"", v))
                                .collect::<Vec<_>>()
                                .join(" | ");
                            lines.push(format!("{}  {}: {}{}", pad, key, enum_str, comma));
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

    /// Generate TypeScript type definition
    pub fn to_typescript(&self, name: &str) -> String {
        let body = self.ts_type(1);
        format!("type {} = {}", name, body)
    }

    fn ts_type(&self, indent: usize) -> String {
        let pad = "  ".repeat(indent);
        let pad_outer = "  ".repeat(indent.saturating_sub(1));
        match self {
            JsonType::Null => "null".to_string(),
            JsonType::Bool => "boolean".to_string(),
            JsonType::Number => "number".to_string(),
            JsonType::String => "string".to_string(),
            JsonType::Buffer => "Buffer".to_string(),
            JsonType::Enum(values) => values.iter()
                .map(|v| format!("\"{}\"", v))
                .collect::<Vec<_>>()
                .join(" | "),
            JsonType::Array(inner) => format!("{}[]", inner.ts_type(indent)),
            JsonType::Object(fields) => {
                if fields.is_empty() {
                    return "{}".to_string();
                }
                let mut lines = vec!["{".to_string()];
                for (key, val) in fields {
                    lines.push(format!("{}{}: {};", pad, key, val.ts_type(indent + 1)));
                }
                lines.push(format!("{}}}", pad_outer));
                lines.join("\n")
            }
        }
    }

    /// Generate C# class definition
    pub fn to_csharp(&self, name: &str) -> String {
        // Unwrap array to get the inner object type
        let obj_fields = match self {
            JsonType::Object(fields) => Some(fields),
            JsonType::Array(inner) => {
                if let JsonType::Object(fields) = inner.as_ref() {
                    Some(fields)
                } else {
                    None
                }
            }
            _ => None,
        };

        let mut lines = vec![format!("public class {}", name)];
        lines.push("{".to_string());
        if let Some(fields) = obj_fields {
            for (key, val) in fields {
                let cs_type = val.csharp_type();
                let prop_name = capitalize(key);
                lines.push(format!("    public {} {} {{ get; set; }}", cs_type, prop_name));
            }
        }
        lines.push("}".to_string());
        lines.join("\n")
    }

    fn csharp_type(&self) -> String {
        match self {
            JsonType::Null => "object?".to_string(),
            JsonType::Bool => "bool".to_string(),
            JsonType::Number => "int".to_string(),
            JsonType::String => "string".to_string(),
            JsonType::Buffer => "byte[]".to_string(),
            JsonType::Enum(_) => "string".to_string(),
            JsonType::Array(inner) => format!("List<{}>", inner.csharp_type()),
            JsonType::Object(fields) => {
                // Inline anonymous object — for nested, would need named classes
                if fields.is_empty() {
                    return "object".to_string();
                }
                "object".to_string()
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
            (JsonType::Buffer, _) => {} // Buffer matches any binary response
            (JsonType::Enum(allowed), serde_json::Value::String(s)) => {
                if !allowed.iter().any(|v| v == s) {
                    mismatches.push(TypeMismatch {
                        path: path.to_string(),
                        expected: format!("one of {:?}", allowed),
                        actual: format!("\"{}\"", s),
                    });
                }
            }
            (JsonType::Enum(allowed), _) => {
                // Enum expects a string value
                let actual_str = match value {
                    serde_json::Value::Number(n) => n.to_string(),
                    serde_json::Value::Bool(b) => b.to_string(),
                    serde_json::Value::Null => "null".to_string(),
                    _ => JsonType::infer(value).label().to_string(),
                };
                if !allowed.iter().any(|v| v == &actual_str) {
                    mismatches.push(TypeMismatch {
                        path: path.to_string(),
                        expected: format!("one of {:?}", allowed),
                        actual: actual_str,
                    });
                }
            }
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

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().to_string() + c.as_str(),
    }
}

/// Parse a TypeScript-like type text back into a JsonType.
///
/// Supports:
/// - `string`, `number`, `boolean`, `null` — primitives
/// - `type[]` — arrays
/// - `{ field: type, ... }` — objects (multi-line)
/// - `"val1" | "val2" | ...` — enums
pub fn parse_type_text(text: &str) -> Result<JsonType, std::string::String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(JsonType::Null);
    }

    let mut parser = TypeParser::new(trimmed);
    parser.parse_type()
}

struct TypeParser<'a> {
    input: &'a str,
    pos: usize,
}

impl<'a> TypeParser<'a> {
    fn new(input: &'a str) -> Self {
        Self { input, pos: 0 }
    }

    fn remaining(&self) -> &'a str {
        &self.input[self.pos..]
    }

    fn skip_whitespace(&mut self) {
        while self.pos < self.input.len() {
            let ch = self.input.as_bytes()[self.pos];
            if ch == b' ' || ch == b'\t' || ch == b'\n' || ch == b'\r' {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn peek(&self) -> Option<u8> {
        self.input.as_bytes().get(self.pos).copied()
    }

    fn parse_type(&mut self) -> Result<JsonType, std::string::String> {
        self.skip_whitespace();

        if self.pos >= self.input.len() {
            return Ok(JsonType::Null);
        }

        match self.peek() {
            Some(b'{') => self.parse_object(),
            Some(b'"') => self.parse_enum_or_string_literal(),
            _ => self.parse_primitive_or_array(),
        }
    }

    fn parse_primitive_or_array(&mut self) -> Result<JsonType, std::string::String> {
        self.skip_whitespace();
        // Read a keyword
        let start = self.pos;
        while self.pos < self.input.len() {
            let ch = self.input.as_bytes()[self.pos];
            if ch.is_ascii_alphanumeric() || ch == b'_' {
                self.pos += 1;
            } else {
                break;
            }
        }
        let keyword = &self.input[start..self.pos];

        // Check for [] suffix
        self.skip_whitespace();
        let is_array = if self.remaining().starts_with("[]") {
            self.pos += 2;
            true
        } else {
            false
        };

        let base = match keyword {
            "string" => JsonType::String,
            "number" => JsonType::Number,
            "boolean" => JsonType::Bool,
            "null" => JsonType::Null,
            "Buffer" => JsonType::Buffer,
            "" => return Err("Expected type keyword".to_string()),
            other => return Err(format!("Unknown type: {}", other)),
        };

        if is_array {
            Ok(JsonType::Array(Box::new(base)))
        } else {
            Ok(base)
        }
    }

    fn parse_enum_or_string_literal(&mut self) -> Result<JsonType, std::string::String> {
        // Parse "value1" | "value2" | ...
        let mut values = Vec::new();

        loop {
            self.skip_whitespace();
            if self.peek() != Some(b'"') {
                break;
            }
            self.pos += 1; // skip opening "
            let start = self.pos;
            while self.pos < self.input.len() && self.input.as_bytes()[self.pos] != b'"' {
                self.pos += 1;
            }
            if self.pos >= self.input.len() {
                return Err("Unterminated string literal in enum".to_string());
            }
            values.push(self.input[start..self.pos].to_string());
            self.pos += 1; // skip closing "

            self.skip_whitespace();
            if self.remaining().starts_with('|') {
                self.pos += 1; // skip |
            } else {
                break;
            }
        }

        if values.is_empty() {
            return Err("Empty enum".to_string());
        }

        Ok(JsonType::Enum(values))
    }

    fn parse_object(&mut self) -> Result<JsonType, std::string::String> {
        self.skip_whitespace();
        if self.peek() != Some(b'{') {
            return Err("Expected '{'".to_string());
        }
        self.pos += 1; // skip {

        let mut fields: Vec<(std::string::String, JsonType)> = Vec::new();

        loop {
            self.skip_whitespace();

            // Check for closing brace
            if self.peek() == Some(b'}') {
                self.pos += 1;
                break;
            }

            if self.pos >= self.input.len() {
                return Err("Unterminated object: expected '}'".to_string());
            }

            // Parse field name
            let name = self.parse_field_name()?;

            // Expect colon
            self.skip_whitespace();
            if self.peek() != Some(b':') {
                return Err(format!("Expected ':' after field name '{}'", name));
            }
            self.pos += 1; // skip :

            // Parse field type
            self.skip_whitespace();
            let field_type = self.parse_type()?;

            fields.push((name, field_type));

            // Skip optional comma
            self.skip_whitespace();
            if self.peek() == Some(b',') {
                self.pos += 1;
            }
        }

        // Check for [] suffix (array of objects)
        self.skip_whitespace();
        if self.remaining().starts_with("[]") {
            self.pos += 2;
            Ok(JsonType::Array(Box::new(JsonType::Object(fields))))
        } else {
            Ok(JsonType::Object(fields))
        }
    }

    fn parse_field_name(&mut self) -> Result<std::string::String, std::string::String> {
        self.skip_whitespace();
        let start = self.pos;
        while self.pos < self.input.len() {
            let ch = self.input.as_bytes()[self.pos];
            if ch.is_ascii_alphanumeric() || ch == b'_' || ch == b'-' || ch == b'.' {
                self.pos += 1;
            } else {
                break;
            }
        }
        let name = &self.input[start..self.pos];
        if name.is_empty() {
            return Err("Expected field name".to_string());
        }
        Ok(name.to_string())
    }
}
