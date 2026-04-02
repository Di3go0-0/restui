use anyhow::Result;
use std::time::{Duration, Instant};

use crate::config::GeneralConfig;
use crate::model::request::{PathParam, Request};
use crate::model::response::Response;
use crate::state::MAX_REDIRECTS;

pub async fn execute(request: &Request, config: &GeneralConfig) -> Result<Response> {
    let url_lower = request.url.to_lowercase();
    if !url_lower.starts_with("http://") && !url_lower.starts_with("https://") {
        return Err(anyhow::anyhow!(
            "Invalid URL scheme: only http:// and https:// are allowed\n\nGot: {}",
            request.url
        ));
    }

    let client = reqwest::Client::builder()
        .redirect(if config.follow_redirects {
            reqwest::redirect::Policy::limited(MAX_REDIRECTS)
        } else {
            reqwest::redirect::Policy::none()
        })
        .danger_accept_invalid_certs(!config.verify_ssl)
        .timeout(Duration::from_secs(config.timeout_secs))
        .build()
        .map_err(|e| anyhow::anyhow!(classify_error(&e)))?;

    let start = Instant::now();

    let method: reqwest::Method = match request.method {
        crate::model::request::HttpMethod::GET => reqwest::Method::GET,
        crate::model::request::HttpMethod::POST => reqwest::Method::POST,
        crate::model::request::HttpMethod::PUT => reqwest::Method::PUT,
        crate::model::request::HttpMethod::PATCH => reqwest::Method::PATCH,
        crate::model::request::HttpMethod::DELETE => reqwest::Method::DELETE,
        crate::model::request::HttpMethod::HEAD => reqwest::Method::HEAD,
        crate::model::request::HttpMethod::OPTIONS => reqwest::Method::OPTIONS,
    };

    let mut builder = client.request(method, &request.url);

    for header in &request.headers {
        if header.enabled {
            builder = builder.header(&header.name, &header.value);
        }
    }

    for param in &request.query_params {
        if param.enabled {
            builder = builder.query(&[(&param.key, &param.value)]);
        }
    }

    // Merge enabled cookies into a single Cookie header
    let cookie_str: String = request
        .cookies
        .iter()
        .filter(|c| c.enabled && !c.name.is_empty())
        .map(|c| format!("{}={}", c.name, c.value))
        .collect::<Vec<_>>()
        .join("; ");
    if !cookie_str.is_empty() {
        builder = builder.header("Cookie", &cookie_str);
    }

    let body = request.any_body();
    if let Some(body) = body {
        builder = builder.body(body.to_string());
    }

    tracing::debug!(method = %request.method, url = %request.url, "Sending HTTP request");
    let resp = builder.send().await.map_err(|e| {
        tracing::warn!(error = %e, url = %request.url, "HTTP request failed");
        anyhow::anyhow!(classify_error(&e))
    })?;
    let elapsed = start.elapsed();

    let status = resp.status().as_u16();
    tracing::debug!(status, elapsed_ms = elapsed.as_millis() as u64, url = %request.url, "HTTP response received");
    let status_text = resp
        .status()
        .canonical_reason()
        .unwrap_or("")
        .to_string();
    let headers: Vec<(String, String)> = resp
        .headers()
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
        .collect();
    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .map(String::from);
    let body_bytes = resp.bytes().await.map_err(|e| anyhow::anyhow!(classify_error(&e)))?;
    let size_bytes = body_bytes.len();

    let is_binary = content_type.as_deref().is_some_and(|ct| {
        ct.starts_with("image/")
            || ct.starts_with("audio/")
            || ct.starts_with("video/")
            || ct == "application/octet-stream"
            || ct == "application/pdf"
            || ct == "application/zip"
    });

    let (body, raw_bytes) = if is_binary {
        let ct_display = content_type.as_deref().unwrap_or("binary");
        let size_display = if size_bytes < 1024 {
            format!("{}B", size_bytes)
        } else if size_bytes < 1024 * 1024 {
            format!("{:.1}KB", size_bytes as f64 / 1024.0)
        } else {
            format!("{:.1}MB", size_bytes as f64 / (1024.0 * 1024.0))
        };
        (
            format!("Binary response ({}, {})", ct_display, size_display),
            Some(body_bytes.to_vec()),
        )
    } else {
        (String::from_utf8_lossy(&body_bytes).to_string(), None)
    };

    Ok(Response {
        status,
        status_text,
        headers,
        body,
        content_type,
        elapsed,
        size_bytes,
        body_bytes: raw_bytes,
    })
}

/// Classify a reqwest error into a human-readable diagnostic message.
fn classify_error(err: &reqwest::Error) -> String {
    let raw = err.to_string();

    if err.is_builder() {
        if raw.contains("invalid URL") || raw.contains("URL scheme") {
            return format!("Invalid URL: check the URL format\n\n{}", raw);
        }
        return format!("Request builder error: {}", raw);
    }

    if err.is_connect() {
        let msg = std::error::Error::source(err).map(|s| s.to_string()).unwrap_or_default();
        if msg.contains("dns") || msg.contains("resolve") || msg.contains("Name or service not known") || raw.contains("dns") {
            return format!("DNS Error: could not resolve host\n\n{}", raw);
        }
        if msg.contains("ssl") || msg.contains("tls") || msg.contains("certificate")
            || msg.contains("SSL") || msg.contains("TLS") || msg.contains("Certificate")
            || raw.contains("certificate") || raw.contains("SSL") {
            return format!("SSL/TLS Error: certificate verification failed\nTip: toggle insecure mode with Ctrl+S\n\n{}", raw);
        }
        if msg.contains("refused") || raw.contains("refused") {
            return format!("Connection Refused: server is not listening on that port\n\n{}", raw);
        }
        return format!("Connection Error: could not reach host\n\n{}", raw);
    }

    if err.is_timeout() {
        return format!("Timeout: server did not respond in time\n\n{}", raw);
    }

    if err.is_decode() {
        return format!("Decode Error: could not read response body\n\n{}", raw);
    }

    if err.is_redirect() {
        return format!("Too Many Redirects: exceeded redirect limit\n\n{}", raw);
    }

    format!("Request Error: {}", raw)
}

pub fn resolve_path_params(url: &str, params: &[PathParam]) -> String {
    let mut result = url.to_string();
    for param in params.iter().filter(|p| p.enabled && !p.key.is_empty()) {
        result = result.replace(&format!(":{}", param.key), &param.value);
        result = result.replace(&format!("{{{}}}", param.key), &param.value);
    }
    result
}

/// Escape a string for safe use inside single quotes in a shell command.
fn shell_escape(s: &str) -> String {
    s.replace('\'', "'\\''")
}

pub fn to_curl(request: &Request) -> String {
    let mut parts = vec![format!("curl -X {}", request.method)];

    for h in &request.headers {
        if h.enabled {
            parts.push(format!("-H '{}: {}'", shell_escape(&h.name), shell_escape(&h.value)));
        }
    }

    // Merge cookies into a Cookie header
    let cookie_str: String = request.cookies.iter()
        .filter(|c| c.enabled && !c.name.is_empty())
        .map(|c| format!("{}={}", c.name, c.value))
        .collect::<Vec<_>>()
        .join("; ");
    if !cookie_str.is_empty() {
        parts.push(format!("-H 'Cookie: {}'", shell_escape(&cookie_str)));
    }

    let curl_body = request.any_body();
    if let Some(body) = curl_body {
        parts.push(format!("-d '{}'", shell_escape(body)));
    }

    let base_url = resolve_path_params(&request.url, &request.path_params);
    let enabled_params: Vec<_> = request.query_params.iter().filter(|p| p.enabled && !p.key.is_empty()).collect();
    let url = if enabled_params.is_empty() {
        base_url
    } else {
        let qs: Vec<String> = enabled_params
            .iter()
            .map(|p| format!("{}={}", percent_encode(&p.key), percent_encode(&p.value)))
            .collect();
        format!("{}?{}", base_url, qs.join("&"))
    };

    parts.push(format!("'{}'", shell_escape(&url)));
    parts.join(" \\\n  ")
}

/// Minimal percent-encoding for query string values in curl output.
fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            ' ' => out.push_str("%20"),
            '&' => out.push_str("%26"),
            '=' => out.push_str("%3D"),
            '+' => out.push_str("%2B"),
            '#' => out.push_str("%23"),
            '%' => out.push_str("%25"),
            _ if c.is_ascii_alphanumeric() || "-._~".contains(c) => out.push(c),
            _ => {
                for b in c.to_string().as_bytes() {
                    out.push_str(&format!("%{:02X}", b));
                }
            }
        }
    }
    out
}
