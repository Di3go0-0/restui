use crate::action::Action;
use crate::http_client;
use crate::model::request::{Header, Request};
use crate::model::response::Response;
use crate::state::RESPONSE_CACHE_MAX;

use super::App;

impl App {
    pub(super) fn cache_response(&mut self, key: String, response: Response) {
        self.state.response_cache.insert(key, (response, std::time::Instant::now()));
        while self.state.response_cache.len() > RESPONSE_CACHE_MAX {
            if let Some(oldest_key) = self.state.response_cache.iter()
                .min_by_key(|(_, (_, ts))| *ts)
                .map(|(k, _)| k.clone())
            {
                self.state.response_cache.remove(&oldest_key);
            } else {
                break;
            }
        }
    }

    pub(super) fn cancel_request(&mut self) {
        if let Some(handle) = self.state.request_abort_handle.take() {
            handle.abort();
        }
        self.state.request_in_flight = false;
        self.state.request_started_at = None;
        self.state.set_status("Request cancelled");
    }

    pub(super) async fn execute_request(&mut self) {
        // Cancel any previous in-flight request
        if let Some(handle) = self.state.request_abort_handle.take() {
            handle.abort();
        }
        self.state.request_in_flight = true;
        self.state.request_started_at = Some(std::time::Instant::now());
        self.state.last_error = None;
        self.state.set_status("Sending request...");
        tracing::debug!(method = %self.state.current_request.method, url = %self.state.current_request.url, "Executing request");

        let mut resolved = self.resolve_env_vars(&self.state.current_request);

        // Resolve chain references {{@request_name.json.path}}
        let mut resolving_stack = Vec::new();
        if let Some(ref name) = self.state.current_request.name {
            let coll_name = self.state.collections
                .get(self.state.active_collection)
                .map(|c| c.name.clone())
                .unwrap_or_default();
            resolving_stack.push(format!("{}/{}", coll_name, name));
        }

        match self.resolve_chains_in_request(&mut resolved, &mut resolving_stack).await {
            Ok(()) => {}
            Err(err) => {
                self.state.request_in_flight = false;
                self.state.last_error = Some(err.clone());
                self.state.set_status(format!("Error: {}", err));
                return;
            }
        }

        // Resolve path params in URL
        resolved.url = http_client::resolve_path_params(&resolved.url, &resolved.path_params);

        // Pick the active body based on body_type
        let body_text_trimmed = resolved.get_body(self.state.body_type).trim().to_string();

        // Auto-inject Content-Type if body exists and no Content-Type header set
        if !body_text_trimmed.is_empty() {
            let has_ct = resolved.headers.iter().any(|h| h.enabled && h.name.eq_ignore_ascii_case("content-type"));
            if !has_ct {
                resolved.headers.push(Header {
                    name: "Content-Type".to_string(),
                    value: self.state.body_type.content_type().to_string(),
                    enabled: true,
                });
            }
        }

        // Clear all body fields, then set only the active one (trimmed, non-empty)
        resolved.body_json = None;
        resolved.body_xml = None;
        resolved.body_form = None;
        resolved.body_raw = None;
        if !body_text_trimmed.is_empty() {
            resolved.set_body(self.state.body_type, Some(body_text_trimmed));
        }

        let config = self.state.config.general.clone();
        let tx = self.action_tx.clone();

        let handle = tokio::spawn(async move {
            match http_client::execute(&resolved, &config).await {
                Ok(resp) => { let _ = tx.send(Action::RequestCompleted(Box::new(resp))); }
                Err(e) => { let _ = tx.send(Action::RequestFailed(e.to_string())); }
            }
        });
        self.state.request_abort_handle = Some(handle.abort_handle());
    }

    pub(super) fn resolve_env_vars(&self, req: &Request) -> Request {
        let env = &self.state.environments;
        Request {
            name: req.name.clone(),
            method: req.method,
            url: env.resolve(&req.url),
            headers: req.headers.iter().map(|h| Header { name: h.name.clone(), value: env.resolve(&h.value), enabled: h.enabled }).collect(),
            query_params: req.query_params.iter().map(|p| crate::model::request::QueryParam { key: p.key.clone(), value: env.resolve(&p.value), enabled: p.enabled }).collect(),
            cookies: req.cookies.iter().map(|c| crate::model::request::Cookie { name: c.name.clone(), value: env.resolve(&c.value), enabled: c.enabled }).collect(),
            path_params: req.path_params.iter().map(|p| crate::model::request::PathParam { key: p.key.clone(), value: env.resolve(&p.value), enabled: p.enabled }).collect(),
            body_json: req.body_json.as_ref().map(|b| env.resolve(b)),
            body_xml: req.body_xml.as_ref().map(|b| env.resolve(b)),
            body_form: req.body_form.as_ref().map(|b| env.resolve(b)),
            body_raw: req.body_raw.as_ref().map(|b| env.resolve(b)),
            source_file: req.source_file.clone(),
            source_line: req.source_line,
        }
    }

    /// Resolve all `{{@...}}` chain references in a request's fields.
    pub(super) fn resolve_chains_in_request<'a>(
        &'a mut self,
        req: &'a mut Request,
        resolving: &'a mut Vec<String>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + 'a>> {
        Box::pin(async move {
        use crate::model::chain::find_chain_refs;

        let has_refs = |s: &str| !find_chain_refs(s).is_empty();
        let need_resolve = has_refs(&req.url)
            || req.headers.iter().any(|h| has_refs(&h.value))
            || req.query_params.iter().any(|p| has_refs(&p.value))
            || req.cookies.iter().any(|c| has_refs(&c.value))
            || req.has_chain_refs_in_body(has_refs);

        if !need_resolve {
            return Ok(());
        }

        self.state.set_status("Resolving dependencies...");

        req.url = self.resolve_chains_in_str(&req.url, resolving).await?;

        for i in 0..req.headers.len() {
            let val = req.headers[i].value.clone();
            req.headers[i].value = self.resolve_chains_in_str(&val, resolving).await?;
        }
        for i in 0..req.query_params.len() {
            let val = req.query_params[i].value.clone();
            req.query_params[i].value = self.resolve_chains_in_str(&val, resolving).await?;
        }
        for i in 0..req.cookies.len() {
            let val = req.cookies[i].value.clone();
            req.cookies[i].value = self.resolve_chains_in_str(&val, resolving).await?;
        }
        for i in 0..req.path_params.len() {
            let val = req.path_params[i].value.clone();
            req.path_params[i].value = self.resolve_chains_in_str(&val, resolving).await?;
        }
        if let Some(ref body) = req.body_json.clone() {
            req.body_json = Some(self.resolve_chains_in_str(body, resolving).await?);
        }
        if let Some(ref body) = req.body_xml.clone() {
            req.body_xml = Some(self.resolve_chains_in_str(body, resolving).await?);
        }
        if let Some(ref body) = req.body_form.clone() {
            req.body_form = Some(self.resolve_chains_in_str(body, resolving).await?);
        }
        if let Some(ref body) = req.body_raw.clone() {
            req.body_raw = Some(self.resolve_chains_in_str(body, resolving).await?);
        }

        Ok(())
        }) // Box::pin
    }

    /// Resolve all `{{@...}}` references in a single string value.
    async fn resolve_chains_in_str(
        &mut self,
        value: &str,
        resolving: &mut Vec<String>,
    ) -> Result<String, String> {
        use crate::model::chain::{find_chain_refs, parse_chain_ref, extract_json_value, ChainError};

        let refs = find_chain_refs(value);
        if refs.is_empty() {
            return Ok(value.to_string());
        }

        let mut result = value.to_string();

        for (start, end, inner) in refs.into_iter().rev() {
            let chain_ref = parse_chain_ref(&inner).ok_or_else(|| {
                format!("Chain error: invalid reference syntax '{{{{@{}}}}}'", inner)
            })?;

            let (coll_idx, dep_request_clone) = {
                let (ci, req) = self.find_request_by_name(
                    &chain_ref.request_name,
                    chain_ref.collection.as_deref(),
                ).ok_or_else(|| {
                    ChainError::RequestNotFound { name: chain_ref.request_name.clone() }.to_string()
                })?;
                (ci, req.clone())
            };

            let coll_name = self.state.collections[coll_idx].name.clone();
            let cache_key = format!("{}/{}", coll_name, chain_ref.request_name);

            if resolving.contains(&cache_key) {
                let mut chain = resolving.clone();
                chain.push(cache_key);
                return Err(ChainError::CircularDependency { chain }.to_string());
            }

            let ttl = std::time::Duration::from_secs(self.state.config.general.chain_cache_ttl);
            let cached_valid = self.state.response_cache.get(&cache_key)
                .is_some_and(|(_, cached_at)| cached_at.elapsed() < ttl);

            if !cached_valid {
                self.state.response_cache.remove(&cache_key);
                self.state.set_status(format!("Resolving: {}...", chain_ref.request_name));

                let mut resolved_dep = self.resolve_env_vars(&dep_request_clone);

                let dep_body = resolved_dep.any_body().unwrap_or("").trim();
                if !dep_body.is_empty() {
                    let has_ct = resolved_dep.headers.iter().any(|h| h.enabled && h.name.eq_ignore_ascii_case("content-type"));
                    if !has_ct {
                        resolved_dep.headers.push(crate::model::request::Header {
                            name: "Content-Type".to_string(),
                            value: "application/json".to_string(),
                            enabled: true,
                        });
                    }
                }

                resolving.push(cache_key.clone());
                self.resolve_chains_in_request(&mut resolved_dep, resolving).await?;
                resolving.pop();

                let config = self.state.config.general.clone();
                let resp = http_client::execute(&resolved_dep, &config).await
                    .map_err(|e| ChainError::DependencyFailed {
                        request_name: chain_ref.request_name.clone(),
                        error: e.to_string(),
                    }.to_string())?;

                if resp.status >= 200 && resp.status < 300 {
                    self.cache_response(cache_key.clone(), resp);
                } else {
                    return Err(format!(
                        "Chain error: dependency '{}' returned {} {}",
                        chain_ref.request_name, resp.status, resp.status_text
                    ));
                }
            }

            let (resp, _) = self.state.response_cache.get(&cache_key).unwrap();
            let extracted = extract_json_value(&resp.body, &chain_ref.json_path)
                .map_err(|e| match e {
                    ChainError::JsonPathNotFound { .. } => {
                        format!("Chain error: path '{}' not found in response from '{}'",
                            chain_ref.json_path, chain_ref.request_name)
                    }
                    other => other.to_string(),
                })?;

            result.replace_range(start..end, &extracted);
        }

        Ok(result)
    }

    /// Find a request by name across collections.
    pub(super) fn find_request_by_name(&self, name: &str, collection: Option<&str>) -> Option<(usize, &Request)> {
        if let Some(coll_name) = collection {
            for (ci, coll) in self.state.collections.iter().enumerate() {
                if coll.name == coll_name {
                    for req in &coll.requests {
                        if req.name.as_deref() == Some(name) {
                            return Some((ci, req));
                        }
                    }
                }
            }
            return None;
        }

        if let Some(coll) = self.state.collections.get(self.state.active_collection) {
            for req in &coll.requests {
                if req.name.as_deref() == Some(name) {
                    return Some((self.state.active_collection, req));
                }
            }
        }

        for (ci, coll) in self.state.collections.iter().enumerate() {
            if ci == self.state.active_collection {
                continue;
            }
            for req in &coll.requests {
                if req.name.as_deref() == Some(name) {
                    return Some((ci, req));
                }
            }
        }

        None
    }
}
