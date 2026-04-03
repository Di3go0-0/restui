use crate::core::state::{ChainAutocomplete, Panel};

use super::App;

impl App {
    pub(super) fn try_chain_autocomplete(&mut self) {
        // Get text and cursor position based on active panel
        let (text, cursor_pos) = match self.state.active_panel {
            Panel::Request => {
                let text = self.get_request_field_text();
                let cursor = self.get_request_cursor();
                (text, cursor)
            }
            Panel::Body => {
                let body = self.active_body().to_string();
                let lines: Vec<&str> = body.lines().collect();
                let line = lines.get(self.state.body_vim.cursor_row).copied().unwrap_or("");
                (line.to_string(), self.state.body_vim.cursor_col)
            }
            _ => {
                self.state.chain_autocomplete = None;
                return;
            }
        };

        // Find {{ or {{@ before cursor
        let before_cursor = &text[..cursor_pos.min(text.len())];

        // Check for {{@ first (chain autocomplete takes priority)
        let at_pos = before_cursor.rfind("{{@");
        let brace_pos = before_cursor.rfind("{{");

        // If we have {{ but not {{@ (or {{ appears after {{@), show env autocomplete
        if let Some(bp) = brace_pos {
            let is_chain = at_pos.is_some_and(|ap| ap == bp); // {{@ starts at same position as {{
            if !is_chain {
                let after_braces = &before_cursor[bp + 2..];
                // Don't show if already closed with }}
                if !after_braces.contains("}}") {
                    let prefix = after_braces.to_lowercase();
                    let prefix_len = after_braces.len();
                    let panel = self.state.active_panel;
                    let mut items: Vec<(String, String)> = Vec::new();

                    if let Some(env) = self.state.environments.active_env() {
                        for (key, value) in &env.variables {
                            if key.to_lowercase().starts_with(&prefix) || prefix.is_empty() {
                                let truncated = if value.len() > 20 {
                                    format!("{}…", &value[..19])
                                } else {
                                    value.clone()
                                };
                                let suffix = &key[prefix_len.min(key.len())..];
                                items.push((
                                    format!("{}: {}", key, truncated),
                                    suffix.to_string(),
                                ));
                            }
                        }
                    }

                    if items.is_empty() {
                        items.push(("(no active environment \u{2014} press p to select)".to_string(), String::new()));
                    }

                    self.state.chain_autocomplete = Some(ChainAutocomplete {
                        items,
                        selected: 0,
                        anchor_panel: panel,
                        kind: crate::core::state::AutocompleteKind::Env,
                    });
                    return;
                }
            }
        }

        if at_pos.is_none() {
            self.state.chain_autocomplete = None;
            return;
        }
        let at_pos = at_pos.unwrap();
        let after_at = &before_cursor[at_pos + 3..]; // everything after {{@

        // Check if we've already closed with }}
        if after_at.contains("}}") {
            self.state.chain_autocomplete = None;
            return;
        }

        let panel = self.state.active_panel;

        // Extract request name and path from after_at
        // Handles: "auth", "auth.", "auth.token", "auth[0]", "auth[0].", "auth[0].token"
        let (request_name_raw, path_so_far, has_path) = if let Some(bracket_pos) = after_at.find('[') {
            // Has bracket: split at first [
            let name = &after_at[..bracket_pos];
            let rest = &after_at[bracket_pos..];
            // rest is like "[0].token" or "[0]." or "[0]"
            (name, rest, true)
        } else if let Some(dot_pos) = after_at.find('.') {
            let name = &after_at[..dot_pos];
            let rest = &after_at[dot_pos + 1..];
            (name, rest, true)
        } else {
            (after_at, "", false)
        };

        if !has_path {
            // Suggest request names matching prefix
            let prefix = request_name_raw.to_lowercase();
            let prefix_len = request_name_raw.len();
            let mut items: Vec<(String, String)> = Vec::new();
            for coll in &self.state.collections {
                for req in &coll.requests {
                    if let Some(ref name) = req.name {
                        if name.to_lowercase().starts_with(&prefix) || prefix.is_empty() {
                            // insert_text is only the suffix (what's missing after what the user already typed)
                            let suffix = &name[prefix_len..];
                            items.push((
                                format!("{} ({})", name, coll.name),
                                suffix.to_string(),
                            ));
                        }
                    }
                }
            }
            if items.is_empty() {
                items.push(("(no named requests)".to_string(), String::new()));
            }
            self.state.chain_autocomplete = Some(ChainAutocomplete {
                items,
                selected: 0,
                anchor_panel: panel,
                kind: crate::core::state::AutocompleteKind::Chain,
            });
        } else {
            // Has path — suggest fields from type
            let request_name = request_name_raw;

            // Find cached response type for this request
            let mut found_type: Option<crate::model::response_type::JsonType> = None;
            for coll in &self.state.collections {
                for req in &coll.requests {
                    if req.name.as_deref() == Some(request_name) {
                        let key = format!("{}/{}", coll.name, request_name);
                        if let Some((resp, _)) = self.state.response_cache.get(&key) {
                            if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(&resp.body) {
                                found_type = Some(crate::model::response_type::JsonType::infer(&json_val));
                            }
                        }
                    }
                }
            }

            let Some(root_type) = found_type else {
                self.state.chain_autocomplete = Some(ChainAutocomplete {
                    items: vec![("(no type \u{2014} execute request first)".to_string(), String::new())],
                    selected: 0,
                    anchor_panel: panel,
                    kind: crate::core::state::AutocompleteKind::Chain,
                });
                return;
            };

            // Navigate type by path segments
            let segments: Vec<&str> = path_so_far.split('.').collect();
            let current_prefix = segments.last().copied().unwrap_or("");

            // Clone root_type to avoid borrow issues, then navigate
            let navigated = Self::navigate_type(&root_type, &segments[..segments.len().saturating_sub(1)]);
            let current_type = match navigated {
                Some(t) => t,
                None => {
                    self.state.chain_autocomplete = Some(ChainAutocomplete {
                        items: vec![("(field not found)".to_string(), String::new())],
                        selected: 0,
                        anchor_panel: panel,
                        kind: crate::core::state::AutocompleteKind::Chain,
                    });
                    return;
                }
            };

            // Build suggestions from current_type's fields
            let mut items: Vec<(String, String)> = Vec::new();

            let prefix_len = current_prefix.len();
            match &current_type {
                crate::model::response_type::JsonType::Object(fields) => {
                    let prefix_lower = current_prefix.to_lowercase();
                    for (key, val_type) in fields {
                        if key.to_lowercase().starts_with(&prefix_lower) || current_prefix.is_empty() {
                            let type_label = val_type.label();
                            // insert_text is only the suffix
                            let suffix = &key[prefix_len..];
                            items.push((
                                format!("{}: {}", key, type_label),
                                suffix.to_string(),
                            ));
                        }
                    }
                }
                crate::model::response_type::JsonType::Array(inner) => {
                    items.push((
                        format!("\u{26a0} Array of {} \u{2014} use [index]", inner.label()),
                        "[0]".to_string(),
                    ));
                    // Also show inner fields if it's an array of objects
                    if let crate::model::response_type::JsonType::Object(fields) = inner.as_ref() {
                        items.push(("\u{2500}\u{2500} after [index]: \u{2500}\u{2500}".to_string(), String::new()));
                        for (key, val_type) in fields {
                            let suffix = &key[prefix_len.min(key.len())..];
                            items.push((
                                format!("  {}: {}", key, val_type.label()),
                                suffix.to_string(),
                            ));
                        }
                    }
                }
                _ => {
                    items.push((format!("(type: {}, no sub-fields)", current_type.label()), String::new()));
                }
            }

            if items.is_empty() {
                items.push(("(no fields available)".to_string(), String::new()));
            }

            self.state.chain_autocomplete = Some(ChainAutocomplete {
                items,
                selected: 0,
                anchor_panel: panel,
                kind: crate::core::state::AutocompleteKind::Chain,
            });
        }
    }

    /// Navigate a JsonType by path segments, returning the type at the end of the path.
    pub(super) fn navigate_type(root: &crate::model::response_type::JsonType, segments: &[&str]) -> Option<crate::model::response_type::JsonType> {
        use crate::model::response_type::JsonType;
        let mut current = root.clone();
        for seg in segments {
            if seg.is_empty() { continue; }

            // Check if this is a pure array index like "[0]"
            let is_array_index = seg.starts_with('[');
            let field_name = seg.split('[').next().unwrap_or(seg);
            let has_index = seg.contains('[');

            if is_array_index {
                // Pure array index: [0] — just descend into array element
                if let JsonType::Array(inner) = &current {
                    current = inner.as_ref().clone();
                } else {
                    return None; // not an array
                }
                continue;
            }

            match &current {
                JsonType::Object(fields) => {
                    if let Some((_, ft)) = fields.iter().find(|(k, _)| k == field_name) {
                        current = ft.clone();
                        // If field has array index (e.g., "items[0]"), descend into element
                        if has_index {
                            if let JsonType::Array(inner) = &current {
                                current = inner.as_ref().clone();
                            }
                        }
                    } else {
                        return None;
                    }
                }
                JsonType::Array(inner) => {
                    // If current is array but segment is a field name, descend first
                    current = inner.as_ref().clone();
                    if let JsonType::Object(fields) = &current {
                        if let Some((_, ft)) = fields.iter().find(|(k, _)| k == field_name) {
                            current = ft.clone();
                        } else {
                            return None;
                        }
                    }
                }
                _ => {
                    return None;
                }
            }
        }
        Some(current)
    }
}
