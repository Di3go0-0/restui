use std::path::PathBuf;

use crate::parser;
use crate::state::{Panel, RequestFocus};

use super::App;

impl App {
    pub fn load_collections(&mut self, dirs: &[PathBuf]) {
        let collections = parser::scan_directories(dirs);
        self.state.collections = collections;
        self.rebuild_collection_items();
        if let Some(collection) = self.state.collections.first() {
            if let Some(req) = collection.requests.first() {
                self.state.current_request = req.clone();
                let body = self.state.current_request.get_body(self.state.body_type).to_string();
                self.state.body_vim.set_content(&body);
            }
        }
    }

    pub(super) fn rebuild_collection_items(&mut self) {
        let filter = self.state.collections_filter.to_lowercase();
        let has_filter = !filter.is_empty();
        let mut items = Vec::new();
        for (ci, collection) in self.state.collections.iter().enumerate() {
            let expanded = self.state.expanded_collections.contains(&ci);

            if has_filter {
                let matching: Vec<usize> = collection.requests.iter().enumerate()
                    .filter(|(_, req)| {
                        req.display_name().to_lowercase().contains(&filter)
                            || req.url.to_lowercase().contains(&filter)
                    })
                    .map(|(i, _)| i)
                    .collect();
                if matching.is_empty() {
                    continue;
                }
                let arrow = if expanded { "▼" } else { "▶" };
                let marker = if ci == self.state.active_collection { "●" } else { "○" };
                items.push(format!("{} {} {}", arrow, marker, collection.display_name()));
                if expanded {
                    let last_match = *matching.last().unwrap();
                    for &ri in &matching {
                        let req = &collection.requests[ri];
                        let branch = if ri == last_match { "└" } else { "├" };
                        items.push(format!("│ {} {} {}", branch, req.method, req.display_name()));
                    }
                }
            } else {
                let arrow = if expanded { "▼" } else { "▶" };
                let marker = if ci == self.state.active_collection { "●" } else { "○" };
                items.push(format!("{} {} {}", arrow, marker, collection.display_name()));
                if expanded {
                    let last_idx = collection.requests.len().saturating_sub(1);
                    for (ri, req) in collection.requests.iter().enumerate() {
                        let branch = if ri == last_idx { "└" } else { "├" };
                        items.push(format!("│ {} {} {}", branch, req.method, req.display_name()));
                    }
                }
            }
        }
        self.state.collection_items = items;
    }

    /// Maps a flat list index to (collection_index, Option<request_index>).
    /// Returns None if out of bounds.
    pub(super) fn flat_idx_to_coll_req(&self, flat_idx: usize) -> Option<(usize, Option<usize>)> {
        let filter = self.state.collections_filter.to_lowercase();
        let has_filter = !filter.is_empty();
        let mut idx = 0;
        for (ci, collection) in self.state.collections.iter().enumerate() {
            if has_filter {
                let has_match = collection.requests.iter().any(|req| {
                    req.display_name().to_lowercase().contains(&filter)
                        || req.url.to_lowercase().contains(&filter)
                });
                if !has_match {
                    continue;
                }
            }
            if idx == flat_idx {
                return Some((ci, None)); // collection header
            }
            idx += 1;
            if self.state.expanded_collections.contains(&ci) {
                for ri in 0..collection.requests.len() {
                    if has_filter {
                        let req = &collection.requests[ri];
                        let name_match = req.display_name().to_lowercase().contains(&filter);
                        let url_match = req.url.to_lowercase().contains(&filter);
                        if !name_match && !url_match {
                            continue;
                        }
                    }
                    if idx == flat_idx {
                        return Some((ci, Some(ri)));
                    }
                    idx += 1;
                }
            }
        }
        None
    }

    pub(super) fn select_request_by_flat_index(&mut self, flat_idx: usize) {
        if let Some((ci, Some(ri))) = self.flat_idx_to_coll_req(flat_idx) {
            if let Some(req) = self.state.collections.get(ci).and_then(|c| c.requests.get(ri)) {
                self.state.current_request = req.clone();
                self.state.current_response = None;
                self.state.last_error = None;
                self.state.active_collection = ci;
                let body = self.state.current_request.get_body(self.state.body_type).to_string();
                self.state.body_vim.set_content(&body);
            }
        }
    }

    pub(super) fn save_current_request_over_selected(&mut self) {
        if let Some(flat_idx) = self.state.collections_state.selected() {
            if let Some((ci, Some(ri))) = self.flat_idx_to_coll_req(flat_idx) {
                if let Some(coll) = self.state.collections.get_mut(ci) {
                    if let Some(req) = coll.requests.get_mut(ri) {
                        *req = self.state.current_request.clone();
                        self.persist_collection(ci);
                        self.state.set_status("Request saved".to_string());
                    }
                }
            }
        }
    }

    pub(super) fn save_current_request_as_new(&mut self) {
        let ci = self.state.active_collection;
        if let Some(coll) = self.state.collections.get_mut(ci) {
            let mut new_req = self.state.current_request.clone();
            let base = new_req.name.clone().unwrap_or_else(|| "New Request".to_string());
            new_req.name = Some(format!("{} (copy)", base));
            coll.requests.push(new_req);
            self.persist_collection(ci);
            self.state.expanded_collections.insert(ci);
            self.rebuild_collection_items();
            self.state.set_status("Request saved as new".to_string());
        }
    }

    pub(super) fn sync_params_from_url(&mut self) {
        let url = &self.state.current_request.url;
        if let Some(query_start) = url.find('?') {
            let query = &url[query_start + 1..];
            let mut new_params = Vec::new();
            for pair in query.split('&') {
                let mut parts = pair.splitn(2, '=');
                let key = parts.next().unwrap_or("").to_string();
                let value = parts.next().unwrap_or("").to_string();
                if !key.is_empty() {
                    // Check if this param already exists, preserve enabled status
                    let enabled = self.state.current_request.query_params.iter()
                        .find(|p| p.key == key)
                        .map(|p| p.enabled)
                        .unwrap_or(true);
                    new_params.push(crate::model::request::QueryParam { key, value, enabled });
                }
            }
            self.state.current_request.query_params = new_params;
        } else {
            self.state.current_request.query_params.clear();
        }
    }

    pub(super) fn persist_collection(&self, idx: usize) {
        if let Some(coll) = self.state.collections.get(idx) {
            let content = crate::parser::http::serialize(&coll.requests);
            if let Err(e) = std::fs::write(&coll.path, &content) {
                eprintln!("Failed to save collection: {}", e);
            }
        }
    }

    pub(super) fn switch_active_collection(&mut self) {
        self.state.expanded_collections.insert(self.state.active_collection);
        self.rebuild_collection_items();
        self.state.collections_state.select(Some(0));
        if let Some(coll) = self.state.collections.get(self.state.active_collection) {
            if let Some(req) = coll.requests.first() {
                self.state.current_request = req.clone();
                self.state.current_response = None;
                self.state.last_error = None;
                let body = self.state.current_request.get_body(self.state.body_type).to_string();
                self.state.body_vim.set_content(&body);
            }
            self.state.set_status(format!("Collection: {}", coll.name));
        }
    }

    pub(super) fn try_open_autocomplete(&mut self) {
        if self.state.active_panel == Panel::Request {
            if let RequestFocus::Header(idx) = self.state.request_focus {
                if self.state.header_edit_field == 0 {
                    if let Some(h) = self.state.current_request.headers.get(idx) {
                        let ac = crate::state::Autocomplete::new(&h.name);
                        self.state.autocomplete = if ac.is_empty() { None } else { Some(ac) };
                    }
                }
            }
        }
    }
}
