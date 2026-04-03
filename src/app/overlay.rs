use anyhow::Result;
use std::path::PathBuf;

use crate::action::Action;
use crate::model::collection::{Collection, FileFormat};
use crate::model::request::{Header, Request};
use crate::state::{InputMode, Overlay, RequestFocus};

use super::App;

impl App {
    pub(super) fn handle_overlay(&mut self, action: Action, _count: usize) -> Result<()> {
        match action {
            Action::OpenOverlay(overlay) => {
                if matches!(overlay, Overlay::EnvironmentSelector) {
                    self.state.env_selector_state.select(Some(self.state.environments.active.unwrap_or(0)));
                }
                if matches!(overlay, Overlay::Help) {
                    self.state.help_scroll = 0;
                }
                self.state.overlay = Some(overlay);
            }
            Action::CloseOverlay => {
                // For EnvironmentEditor: if editing, cancel edit instead of closing
                if let Some(Overlay::EnvironmentEditor { cursor, editing_key, .. }) = &self.state.overlay {
                    if *cursor > 0 || *editing_key {
                        if let Some(Overlay::EnvironmentEditor { ref mut cursor, ref mut editing_key, ref mut new_key, ref mut new_value, .. }) = self.state.overlay {
                            *cursor = 0;
                            *editing_key = false;
                            *new_key = String::new();
                            *new_value = String::new();
                        }
                        return Ok(());
                    }
                }
                self.state.overlay = None;
            }
            Action::OverlayUp => {
                match &mut self.state.overlay {
                    Some(Overlay::EnvironmentSelector) => {
                        let i = self.state.env_selector_state.selected().unwrap_or(0).saturating_sub(1);
                        self.state.env_selector_state.select(Some(i));
                    }
                    Some(Overlay::HeaderAutocomplete { selected, .. }) => { *selected = selected.saturating_sub(1); }
                    Some(Overlay::MoveRequest { selected }) => { *selected = selected.saturating_sub(1); }
                    Some(Overlay::ThemeSelector { selected }) => { *selected = selected.saturating_sub(1); }
                    Some(Overlay::History { selected }) => { *selected = selected.saturating_sub(1); }
                    Some(Overlay::ResponseHistory { selected }) => { *selected = selected.saturating_sub(1); }
                    Some(Overlay::ResponseDiffSelect { selected }) => { *selected = selected.saturating_sub(1); }
                    Some(Overlay::Help) => { self.state.help_scroll = self.state.help_scroll.saturating_sub(1); }
                    Some(Overlay::EnvironmentEditor { selected, cursor, .. }) if *cursor == 0 => {
                        *selected = selected.saturating_sub(1);
                    }
                    _ => {}
                }
            }
            Action::OverlayDown => {
                match &mut self.state.overlay {
                    Some(Overlay::EnvironmentSelector) => {
                        let i = self.state.env_selector_state.selected().map(|i| i + 1).unwrap_or(0);
                        let max = self.state.environments.environments.len().saturating_sub(1);
                        self.state.env_selector_state.select(Some(i.min(max)));
                    }
                    Some(Overlay::HeaderAutocomplete { selected, suggestions }) => {
                        *selected = (*selected + 1).min(suggestions.len().saturating_sub(1));
                    }
                    Some(Overlay::MoveRequest { selected }) => {
                        let max = self.state.collections.len().saturating_sub(1);
                        *selected = (*selected + 1).min(max);
                    }
                    Some(Overlay::ThemeSelector { selected }) => {
                        let max = crate::theme::THEME_NAMES.len().saturating_sub(1);
                        *selected = (*selected + 1).min(max);
                    }
                    Some(Overlay::History { selected }) => {
                        let max = self.state.history.entries.len().saturating_sub(1);
                        *selected = (*selected + 1).min(max);
                    }
                    Some(Overlay::ResponseHistory { selected }) => {
                        let key = self.state.current_request.name.as_ref().map(|name| {
                            let coll = self.state.collections.get(self.state.active_collection).map(|c| c.name.as_str()).unwrap_or("_");
                            format!("{}/{}", coll, name)
                        });
                        let max = key.and_then(|k| self.state.response_histories.data.get(&k).map(|h| h.len())).unwrap_or(0usize).saturating_sub(1);
                        *selected = (*selected + 1).min(max);
                    }
                    Some(Overlay::ResponseDiffSelect { selected }) => {
                        let key = self.state.current_request.name.as_ref().map(|name| {
                            let coll = self.state.collections.get(self.state.active_collection).map(|c| c.name.as_str()).unwrap_or("_");
                            format!("{}/{}", coll, name)
                        });
                        let max = key.and_then(|k| self.state.response_histories.data.get(&k).map(|h| h.len())).unwrap_or(0usize).saturating_sub(1);
                        *selected = (*selected + 1).min(max);
                    }
                    Some(Overlay::Help) => { self.state.help_scroll += 1; }
                    Some(Overlay::EnvironmentEditor { selected, cursor, .. }) if *cursor == 0 => {
                        if let Some(active_idx) = self.state.environments.active {
                            let max = self.state.environments.environments[active_idx].variables.len().saturating_sub(1);
                            *selected = (*selected + 1).min(max);
                        }
                    }
                    _ => {}
                }
            }
            Action::OverlayConfirm => {
                let overlay = self.state.overlay.take();
                match overlay {
                    Some(Overlay::EnvironmentSelector) => {
                        if let Some(idx) = self.state.env_selector_state.selected() {
                            if idx < self.state.environments.environments.len() {
                                self.state.environments.active = Some(idx);
                                let name = self.state.environments.environments[idx].name.clone();
                                self.state.set_status(format!("Environment: {}", name));
                            }
                        }
                    }
                    Some(Overlay::HeaderAutocomplete { suggestions, selected }) => {
                        if let Some((name, value)) = suggestions.get(selected) {
                            self.state.current_request.headers.push(Header { name: name.clone(), value: value.clone(), enabled: true });
                            let idx = self.state.current_request.headers.len() - 1;
                            self.state.request_focus = RequestFocus::Header(idx);
                            self.state.header_edit_field = 1;
                            self.state.header_edit_cursor = value.len();
                            self.state.mode = InputMode::Insert;
                        }
                    }
                    Some(Overlay::NewCollection { name }) => {
                        if !name.trim().is_empty() {
                            let filename = format!("{}.http", name.trim());
                            // Create in .http/ folder (convention)
                            let http_dir = PathBuf::from(".http");
                            let _ = std::fs::create_dir_all(&http_dir);
                            let path = http_dir.join(&filename);
                            let content = format!("### {}\nGET https://example.com\n", name.trim());
                            let _ = std::fs::write(&path, &content);
                            if let Ok(requests) = crate::parser::http::parse(&content) {
                                self.state.collections.push(Collection { name: name.trim().to_string(), path, requests, format: FileFormat::Http });
                                self.state.active_collection = self.state.collections.len() - 1;
                                self.rebuild_collection_items();
                                self.state.set_status(format!("Created: .http/{}", filename));
                            }
                        }
                    }
                    Some(Overlay::RenameRequest { name }) => {
                        if !name.trim().is_empty() {
                            if let Some(flat_idx) = self.state.collections_state.selected() {
                                match self.flat_idx_to_coll_req(flat_idx) {
                                    Some((ci, None)) => {
                                        // Rename collection file
                                        if let Some(coll) = self.state.collections.get_mut(ci) {
                                            let old_path = coll.path.clone();
                                            let new_filename = format!("{}.http", name.trim());
                                            let new_path = old_path.with_file_name(&new_filename);
                                            if std::fs::rename(&old_path, &new_path).is_ok() {
                                                coll.name = name.trim().to_string();
                                                coll.path = new_path;
                                                self.rebuild_collection_items();
                                                self.state.set_status(format!("Renamed → '{}'", name.trim()));
                                            } else {
                                                self.state.set_status("Failed to rename file");
                                            }
                                        }
                                    }
                                    Some((ci, Some(ri))) => {
                                        if let Some(req) = self.state.collections.get_mut(ci).and_then(|c| c.requests.get_mut(ri)) {
                                            req.name = Some(name.trim().to_string());
                                            self.state.current_request.name = Some(name.trim().to_string());
                                            self.persist_collection(ci);
                                            self.rebuild_collection_items();
                                            self.state.set_status(format!("Renamed → '{}'", name.trim()));
                                        }
                                    }
                                    None => {}
                                }
                            }
                        }
                    }
                    Some(Overlay::ConfirmDelete { .. }) => {
                        if let Some(flat_idx) = self.state.collections_state.selected() {
                            match self.flat_idx_to_coll_req(flat_idx) {
                                Some((ci, None)) => {
                                    // Delete entire collection
                                    if let Some(coll) = self.state.collections.get(ci) {
                                        let _ = std::fs::remove_file(&coll.path);
                                        let coll_name = coll.name.clone();
                                        self.state.expanded_collections.remove(&ci);
                                        self.state.collections.remove(ci);
                                        if self.state.active_collection >= self.state.collections.len() && self.state.active_collection > 0 {
                                            self.state.active_collection -= 1;
                                        }
                                        self.rebuild_collection_items();
                                        self.state.collections_state.select(Some(0));
                                        if let Some(coll) = self.state.collections.get(self.state.active_collection) {
                                            if let Some(req) = coll.requests.first() {
                                                self.state.current_request = req.clone();
                                            }
                                        } else {
                                            self.state.current_request = Request::default();
                                        }
                                        let body = self.state.current_request.get_body(self.state.body_type).to_string();
                                        self.state.body_vim.set_content(&body);
                                        self.state.current_response = None;
                                        self.state.set_status(format!("Deleted collection '{}'", coll_name));
                                    }
                                }
                                Some((ci, Some(ri))) => {
                                    if let Some(coll) = self.state.collections.get_mut(ci) {
                                        if ri < coll.requests.len() {
                                            let req_name = coll.requests[ri].display_name();
                                            coll.requests.remove(ri);
                                            self.persist_collection(ci);
                                            self.rebuild_collection_items();
                                            let max = self.state.collection_items.len().saturating_sub(1);
                                            self.state.collections_state.select(Some(flat_idx.min(max)));
                                            self.state.set_status(format!("Deleted '{}'", req_name));
                                        }
                                    }
                                }
                                None => {}
                            }
                        }
                    }
                    Some(Overlay::MoveRequest { selected: target_coll }) => {
                        if let Some(flat_idx) = self.state.collections_state.selected() {
                            if let Some((src_ci, Some(ri))) = self.flat_idx_to_coll_req(flat_idx) {
                                if target_coll != src_ci {
                                    if let Some(req) = self.state.collections.get(src_ci).and_then(|c| c.requests.get(ri)).cloned() {
                                        let req_name = req.display_name();
                                        self.state.collections.get_mut(src_ci).unwrap().requests.remove(ri);
                                        self.persist_collection(src_ci);
                                        let target_name = self.state.collections.get(target_coll).map(|c| c.name.clone()).unwrap_or_default();
                                        self.state.collections.get_mut(target_coll).unwrap().requests.push(req);
                                        self.persist_collection(target_coll);
                                        self.state.expanded_collections.insert(target_coll);
                                        self.rebuild_collection_items();
                                        self.state.set_status(format!("Moved '{}' → '{}'", req_name, target_name));
                                    }
                                } else {
                                    self.state.set_status("Cannot move to same collection");
                                }
                            }
                        }
                    }
                    Some(Overlay::ThemeSelector { selected }) => {
                        if let Some(&name) = crate::theme::THEME_NAMES.get(selected) {
                            self.state.theme = crate::theme::load_theme(name);
                            self.state.set_status(format!("Theme: {}", name));
                        }
                    }
                    Some(Overlay::EnvironmentEditor { selected, editing_key, new_key, new_value, cursor }) => {
                        if let Some(active_idx) = self.state.environments.active {
                            if editing_key {
                                // Was adding a new variable: key phase done, now enter value phase
                                if !new_key.is_empty() {
                                    // Switch to value editing phase
                                    self.state.overlay = Some(Overlay::EnvironmentEditor {
                                        selected,
                                        editing_key: false,
                                        new_key: new_key.clone(),
                                        new_value: String::new(),
                                        cursor: 1, // non-zero = editing value
                                    });
                                    return Ok(());
                                }
                            } else if cursor > 0 && !new_key.is_empty() {
                                // Adding new variable: value phase done
                                self.state.environments.environments[active_idx].variables.insert(new_key.clone(), new_value.clone());
                                self.state.set_status(format!("Added: {} = {}", new_key, new_value));
                            } else if cursor > 0 {
                                // Was editing an existing value
                                let env = &mut self.state.environments.environments[active_idx];
                                if let Some((key, val)) = env.variables.get_index_mut(selected) {
                                    let key_name = key.clone();
                                    *val = new_value.clone();
                                    self.state.set_status(format!("Updated: {}", key_name));
                                }
                            } else {
                                // Not editing yet: start editing the selected variable's value
                                let env = &self.state.environments.environments[active_idx];
                                if let Some((_key, val)) = env.variables.get_index(selected) {
                                    let val_clone = val.clone();
                                    let val_len = val_clone.len();
                                    self.state.overlay = Some(Overlay::EnvironmentEditor {
                                        selected,
                                        editing_key: false,
                                        new_key: String::new(),
                                        new_value: val_clone,
                                        cursor: val_len + 1, // non-zero = editing
                                    });
                                    return Ok(());
                                }
                            }
                        }
                    }
                    Some(Overlay::History { selected }) => {
                        // Load selected history entry into current request fields
                        if let Some(entry) = self.state.history.entries.get(selected) {
                            self.state.current_request.method = entry.method;
                            self.state.current_request.url = entry.url.clone();
                            if entry.name.is_some() {
                                self.state.current_request.name = entry.name.clone();
                            }
                            self.state.current_response = None;
                            self.state.last_error = None;
                            self.state.set_status(format!("Loaded: {} {}", entry.method, entry.url));
                        }
                    }
                    Some(Overlay::SetCacheTTL { input }) => {
                        if let Ok(secs) = input.parse::<u64>() {
                            if secs > 0 {
                                self.state.config.general.chain_cache_ttl = secs;
                                self.state.response_cache.clear();
                                self.state.set_status(format!("Chain cache TTL: {}s", secs));
                            } else {
                                self.state.set_status("TTL must be > 0");
                            }
                        } else {
                            self.state.set_status("Invalid number");
                        }
                    }
                    Some(Overlay::ResponseDiffSelect { selected }) => {
                        // Diff current response vs selected historical response
                        if let Some(ref current) = self.state.current_response {
                            if let Some(ref name) = self.state.current_request.name {
                                let collection_name = self.state.collections
                                    .get(self.state.active_collection)
                                    .map(|c| c.name.as_str())
                                    .unwrap_or("_");
                                let key = format!("{}/{}", collection_name, name);
                                if let Some(history) = self.state.response_histories.data.get(&key) {
                                    if let Some(entry) = history.get(selected) {
                                        let current_body = current.formatted_body();
                                        let old_body = entry.response.formatted_body();
                                        let diff = similar::TextDiff::from_lines(&old_body, &current_body);
                                        let mut diff_text = String::new();
                                        for change in diff.iter_all_changes() {
                                            let prefix = match change.tag() {
                                                similar::ChangeTag::Equal => "  ",
                                                similar::ChangeTag::Insert => "+ ",
                                                similar::ChangeTag::Delete => "- ",
                                            };
                                            diff_text.push_str(prefix);
                                            diff_text.push_str(change.to_string_lossy().trim_end_matches('\n'));
                                            diff_text.push('\n');
                                        }
                                        let ts = entry.timestamp.format("%H:%M:%S").to_string();
                                        self.state.resp_vim.set_content(&diff_text);
                                        self.state.viewing_diff = Some((diff_text, ts));
                                        self.state.resp_hscroll = 0;
                                    }
                                }
                            }
                        }
                    }
                    Some(Overlay::ResponseHistory { selected }) => {
                        // Load selected historical response
                        if let Some(ref name) = self.state.current_request.name {
                            let collection_name = self.state.collections
                                .get(self.state.active_collection)
                                .map(|c| c.name.as_str())
                                .unwrap_or("_");
                            let key = format!("{}/{}", collection_name, name);
                            if let Some(history) = self.state.response_histories.data.get(&key) {
                                if let Some(entry) = history.get(selected) {
                                    self.state.current_response = Some(entry.response.clone());
                                    self.state.resp_vim.scroll_offset = 0; self.state.resp_hscroll = 0;
                                    // Re-infer type
                                    if let Some(ref resp) = self.state.current_response {
                                        if resp.body_bytes.is_some() {
                                            self.state.response_type = Some(crate::model::response_type::JsonType::Buffer);
                                        } else if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(&resp.body) {
                                            self.state.response_type = Some(crate::model::response_type::JsonType::infer(&json_val));
                                        } else {
                                            self.state.response_type = None;
                                        }
                                    }
                                    let total = history.len();
                                    let ts = entry.timestamp.format("%H:%M:%S").to_string();
                                    self.state.viewing_history = Some((selected + 1, total, ts));
                                    self.state.set_status(format!("History {}/{} — {}", selected + 1, total, entry.timestamp.format("%H:%M:%S")));
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            Action::OverlayInput(c) => {
                match self.state.overlay {
                    Some(Overlay::NewCollection { ref mut name }) => { name.push(c); }
                    Some(Overlay::RenameRequest { ref mut name }) => { name.push(c); }
                    Some(Overlay::SetCacheTTL { ref mut input }) => {
                        if c.is_ascii_digit() { input.push(c); }
                    }
                    Some(Overlay::EnvironmentEditor { ref mut editing_key, ref mut new_key, ref mut new_value, ref mut cursor, .. }) => {
                        if *cursor == 0 && !*editing_key && c == 'a' {
                            // Start adding a new variable: enter key input mode
                            *editing_key = true;
                            *new_key = String::new();
                            *new_value = String::new();
                            *cursor = 1;
                        } else if *editing_key {
                            // Typing the key name
                            new_key.push(c);
                        } else if *cursor > 0 {
                            // Typing the value
                            new_value.push(c);
                        }
                    }
                    _ => {}
                }
            }
            Action::OverlayBackspace => {
                match self.state.overlay {
                    Some(Overlay::NewCollection { ref mut name }) => { name.pop(); }
                    Some(Overlay::RenameRequest { ref mut name }) => { name.pop(); }
                    Some(Overlay::SetCacheTTL { ref mut input }) => { input.pop(); }
                    Some(Overlay::EnvironmentEditor { ref mut editing_key, ref mut new_key, ref mut new_value, ref mut cursor, .. }) => {
                        if *editing_key {
                            new_key.pop();
                        } else if *cursor > 0 {
                            new_value.pop();
                        }
                    }
                    _ => {}
                }
            }
            Action::OverlayDelete => {
                if let Some(Overlay::EnvironmentEditor { selected, .. }) = &self.state.overlay {
                    let selected = *selected;
                    if let Some(active_idx) = self.state.environments.active {
                        let env = &mut self.state.environments.environments[active_idx];
                        if selected < env.variables.len() {
                            let key = env.variables.get_index(selected).map(|(k, _)| k.clone());
                            if let Some(key) = key {
                                env.variables.shift_remove(&key);
                                self.state.set_status(format!("Deleted: {}", key));
                                // Adjust selected index
                                if let Some(Overlay::EnvironmentEditor { selected: ref mut sel, .. }) = self.state.overlay {
                                    let max = self.state.environments.environments[active_idx].variables.len().saturating_sub(1);
                                    *sel = (*sel).min(max);
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }
}
