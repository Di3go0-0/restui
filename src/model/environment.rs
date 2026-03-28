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
}
