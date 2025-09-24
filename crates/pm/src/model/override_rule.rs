use super::super::helper::package::parse_package_spec;
use super::super::util::json::merge_json_objects;
use super::super::util::registry::resolve;
use super::super::util::semver::{is_valid_version, matches};
use serde_json::Value;

#[derive(Debug, Clone)]
pub struct OverrideRule {
    pub name: String,
    pub spec: String,
    pub target_spec: String,
    pub parent: Option<Box<OverrideRule>>,
}

#[derive(Debug)]
pub struct Overrides {
    pub package: Value,
    pub rules: Vec<OverrideRule>,
}

impl Overrides {
    pub fn new(pkg: Value) -> Self {
        Self {
            package: pkg,
            rules: vec![],
        }
    }

    pub fn parse(pkg: Value) -> Option<Self> {
        let overrides = pkg.get("overrides").and_then(|v| v.as_object());
        let resolutions = pkg.get("resolutions").and_then(|v| v.as_object());
        let merged = merge_json_objects(overrides, resolutions);

        if merged.is_empty() {
            return Some(Self {
                package: pkg,
                rules: vec![],
            });
        }

        let merged_value = Value::Object(merged);
        let mut rules = Vec::new();
        let parser = Self::new(pkg.clone());
        parser.parse_rules(&merged_value, None, &mut rules);
        Some(Self {
            package: pkg,
            rules,
        })
    }

    fn parse_rules(
        &self,
        value: &Value,
        parent: Option<Box<OverrideRule>>,
        rules: &mut Vec<OverrideRule>,
    ) {
        if let Some(obj) = value.as_object() {
            for (key, value) in obj {
                if key == "." {
                    if let Some(parent) = parent.as_ref() {
                        let mut new_parent = parent.clone();
                        new_parent.target_spec = self.parse_target_spec(value);
                        rules.push(*new_parent);
                    }
                    continue;
                }

                let (name, spec) = parse_package_spec(key);
                if value.is_object() {
                    let parent_rule = Box::new(OverrideRule {
                        name: name.to_string(),
                        spec: spec.to_string(),
                        target_spec: String::from("*"),
                        parent: parent.clone(),
                    });

                    self.parse_rules(value, Some(parent_rule), rules);
                } else {
                    rules.push(OverrideRule {
                        name: name.to_string(),
                        spec: spec.to_string(),
                        target_spec: self.parse_target_spec(value),
                        parent: parent.clone(),
                    });
                }
            }
        }
    }

    fn parse_target_spec(&self, value: &Value) -> String {
        match value {
            Value::String(s) if s.starts_with('$') => {
                let dep_name = &s[1..];
                self.package
                    .get("dependencies")
                    .and_then(|deps| deps.get(dep_name))
                    .or_else(|| {
                        self.package
                            .get("devDependencies")
                            .and_then(|d| d.get(dep_name))
                    })
                    .and_then(|v| v.as_str())
                    .unwrap_or("*")
                    .to_string()
            }
            Value::String(s) => s.clone(),
            _ => String::from("*"),
        }
    }

    pub async fn matches_rule(
        &self,
        rule: &OverrideRule,
        dep_name: &str,
        dep_version: &str,
        parent_chain: &[(String, String)],
    ) -> bool {
        if rule.name != dep_name {
            return false;
        }

        if rule.spec != "*" {
            let resolved_version = if is_valid_version(dep_version) {
                dep_version.to_string()
            } else {
                match resolve(dep_name, dep_version).await {
                    Ok(pkg) => pkg.version,
                    Err(_) => return false,
                }
            };

            if !matches(&rule.spec, &resolved_version) {
                return false;
            }
        }

        if let Some(mut current_rule) = rule.parent.as_ref() {
            let mut parent_idx = 0;

            while let Some((parent_name, parent_version)) = parent_chain.get(parent_idx) {
                if parent_name == &current_rule.name {
                    let matches = if current_rule.spec == "*" {
                        true
                    } else {
                        matches(&current_rule.spec, parent_version)
                    };

                    if matches {
                        if let Some(next_rule) = current_rule.parent.as_ref() {
                            current_rule = next_rule;
                            parent_idx += 1;
                            continue;
                        } else {
                            return true;
                        }
                    }
                }
                parent_idx += 1;
            }

            if current_rule.parent.is_some() {
                return false;
            }
        }

        true
    }
}
