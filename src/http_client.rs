use anyhow::Result;
use std::time::{Duration, Instant};

use crate::config::GeneralConfig;
use crate::model::request::Request;
use crate::model::response::Response;

pub async fn execute(request: &Request, config: &GeneralConfig) -> Result<Response> {
    let client = reqwest::Client::builder()
        .redirect(if config.follow_redirects {
            reqwest::redirect::Policy::limited(10)
        } else {
            reqwest::redirect::Policy::none()
        })
        .danger_accept_invalid_certs(!config.verify_ssl)
        .timeout(Duration::from_secs(config.timeout_secs))
        .build()?;

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

    if let Some(ref body) = request.body {
        builder = builder.body(body.clone());
    }

    let resp = builder.send().await?;
    let elapsed = start.elapsed();

    let status = resp.status().as_u16();
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
    let body_bytes = resp.bytes().await?;
    let size_bytes = body_bytes.len();
    let body = String::from_utf8_lossy(&body_bytes).to_string();

    Ok(Response {
        status,
        status_text,
        headers,
        body,
        content_type,
        elapsed,
        size_bytes,
    })
}

pub fn to_curl(request: &Request) -> String {
    let mut parts = vec![format!("curl -X {}", request.method)];

    for h in &request.headers {
        if h.enabled {
            parts.push(format!("-H '{}: {}'", h.name, h.value));
        }
    }

    if let Some(ref body) = request.body {
        let escaped = body.replace('\'', "'\\''");
        parts.push(format!("-d '{}'", escaped));
    }

    let enabled_params: Vec<_> = request.query_params.iter().filter(|p| p.enabled && !p.key.is_empty()).collect();
    let url = if enabled_params.is_empty() {
        request.url.clone()
    } else {
        let qs: Vec<String> = enabled_params
            .iter()
            .map(|p| format!("{}={}", percent_encode(&p.key), percent_encode(&p.value)))
            .collect();
        format!("{}?{}", request.url, qs.join("&"))
    };

    parts.push(format!("'{}'", url));
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
