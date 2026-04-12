use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Environment {
    pub name: String,
    pub variables: IndexMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct EnvironmentStore {
    pub environments: Vec<Environment>,
    pub active: Option<usize>,
}

impl Default for EnvironmentStore {
    fn default() -> Self {
        Self {
            environments: Vec::new(),
            active: None,
        }
    }
}

impl EnvironmentStore {
    pub fn active_env(&self) -> Option<&Environment> {
        self.active.and_then(|i| self.environments.get(i))
    }

    pub fn active_name(&self) -> &str {
        self.active_env()
            .map(|e| e.name.as_str())
            .unwrap_or("none")
    }

    pub fn resolve(&self, template: &str) -> String {
        let Some(env) = self.active_env() else {
            return template.to_string();
        };
        let mut result = template.to_string();
        for (key, value) in &env.variables {
            result = result.replace(&format!("{{{{{}}}}}", key), value);
        }
        result
    }

    /// Resolve variables and return a list of undefined variables found
    pub fn resolve_with_validation(&self, template: &str) -> (String, Vec<String>) {
        let Some(env) = self.active_env() else {
            return (template.to_string(), Vec::new());
        };
        let mut result = template.to_string();
        let mut undefined = Vec::new();

        // Find all {{...}} patterns
        let mut start = 0;
        while let Some(pos) = result[start..].find("{{") {
            let abs_pos = start + pos;
            if let Some(end_pos) = result[abs_pos + 2..].find("}}") {
                let abs_end = abs_pos + 2 + end_pos;
                let var_name = &result[abs_pos + 2..abs_end];
                
                if let Some(value) = env.variables.get(var_name) {
                    result.replace_range(abs_pos..abs_end + 2, value);
                    start = abs_pos + value.len();
                } else {
                    // Variable not found
                    if !undefined.contains(&var_name.to_string()) {
                        undefined.push(var_name.to_string());
                    }
                    start = abs_end + 2;
                }
            } else {
                break;
            }
        }
        (result, undefined)
    }
}
