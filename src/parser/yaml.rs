use anyhow::Result;
use serde::Deserialize;

use crate::model::request::{Header, HttpMethod, QueryParam, Request};

#[derive(Debug, Deserialize)]
struct YamlFile {
    #[serde(default)]
    requests: Vec<YamlRequest>,
}

#[derive(Debug, Deserialize)]
struct YamlRequest {
    name: Option<String>,
    method: String,
    url: String,
    #[serde(default)]
    headers: std::collections::HashMap<String, String>,
    #[serde(default)]
    params: std::collections::HashMap<String, String>,
    body: Option<serde_yaml::Value>,
}

pub fn parse(input: &str) -> Result<Vec<Request>> {
    let file: YamlFile = serde_yaml::from_str(input)?;

    let requests = file
        .requests
        .into_iter()
        .filter_map(|yr| {
            let method = HttpMethod::from_str(&yr.method)?;
            let headers = yr
                .headers
                .into_iter()
                .map(|(name, value)| Header {
                    name,
                    value,
                    enabled: true,
                })
                .collect();
            let query_params = yr
                .params
                .into_iter()
                .map(|(key, value)| QueryParam {
                    key,
                    value,
                    enabled: true,
                })
                .collect();

            let body = yr.body.and_then(|v| match v {
                serde_yaml::Value::String(s) => Some(s),
                other => serde_yaml::to_string(&other).ok(),
            });

            Some(Request {
                name: yr.name,
                method,
                url: yr.url,
                headers,
                query_params,
                cookies: Vec::new(),
                body,
                source_file: None,
                source_line: None,
            })
        })
        .collect();

    Ok(requests)
}
