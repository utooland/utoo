//! Override rules parsing for npm overrides/resolutions.

use super::util::{merge_json_objects, parse_package_spec};
use serde_json::Value;

/// A single override rule specifying version replacement.
#[derive(Debug, Clone)]
pub struct OverrideRule {
    /// Package name to override
    pub name: String,
    /// Version spec to match (or "*" for all versions)
    pub spec: String,
    /// Target version spec to use instead
    pub target_spec: String,
    /// Parent context (for nested overrides)
    pub parent: Option<Box<OverrideRule>>,
}

/// Collection of override rules parsed from package.json.
#[derive(Debug, Clone)]
pub struct Overrides {
    /// Original package.json for reference resolution
    pub package: Value,
    /// Parsed override rules
    pub rules: Vec<OverrideRule>,
}

impl Overrides {
    /// Create a new Overrides instance with no rules.
    pub fn new(pkg: Value) -> Self {
        Self {
            package: pkg,
            rules: vec![],
        }
    }

    /// Parse overrides and resolutions from package.json.
    ///
    /// Supports both npm's `overrides` and yarn's `resolutions` fields.
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
                    // Self-reference: override the parent package itself
                    if let Some(parent) = parent.as_ref() {
                        let mut new_parent = parent.clone();
                        new_parent.target_spec = self.parse_target_spec(value);
                        rules.push(*new_parent);
                    }
                    continue;
                }

                let (name, spec) = parse_package_spec(key);
                if value.is_object() {
                    // Nested override: create parent context
                    let parent_rule = Box::new(OverrideRule {
                        name: name.to_string(),
                        spec: spec.to_string(),
                        target_spec: String::from("*"),
                        parent: parent.clone(),
                    });

                    self.parse_rules(value, Some(parent_rule), rules);
                } else {
                    // Leaf override: create rule
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
            // Reference to project dependency: $dep_name
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_parse_simple_overrides() {
        let pkg = json!({
            "name": "test",
            "overrides": {
                "lodash": "4.17.21"
            }
        });

        let overrides = Overrides::parse(pkg).unwrap();
        assert_eq!(overrides.rules.len(), 1);
        assert_eq!(overrides.rules[0].name, "lodash");
        assert_eq!(overrides.rules[0].spec, "*");
        assert_eq!(overrides.rules[0].target_spec, "4.17.21");
    }

    #[test]
    fn test_parse_versioned_overrides() {
        let pkg = json!({
            "name": "test",
            "overrides": {
                "lodash@^3.0.0": "4.17.21"
            }
        });

        let overrides = Overrides::parse(pkg).unwrap();
        assert_eq!(overrides.rules.len(), 1);
        assert_eq!(overrides.rules[0].name, "lodash");
        assert_eq!(overrides.rules[0].spec, "^3.0.0");
        assert_eq!(overrides.rules[0].target_spec, "4.17.21");
    }

    #[test]
    fn test_parse_reference_overrides() {
        let pkg = json!({
            "name": "test",
            "dependencies": {
                "lodash": "^4.17.0"
            },
            "overrides": {
                "lodash": "$lodash"
            }
        });

        let overrides = Overrides::parse(pkg).unwrap();
        assert_eq!(overrides.rules.len(), 1);
        assert_eq!(overrides.rules[0].target_spec, "^4.17.0");
    }

    #[test]
    fn test_parse_nested_overrides() {
        let pkg = json!({
            "name": "test",
            "overrides": {
                "express": {
                    "debug": "4.0.0"
                }
            }
        });

        let overrides = Overrides::parse(pkg).unwrap();
        assert_eq!(overrides.rules.len(), 1);
        assert_eq!(overrides.rules[0].name, "debug");
        assert_eq!(overrides.rules[0].target_spec, "4.0.0");
        assert!(overrides.rules[0].parent.is_some());
        assert_eq!(overrides.rules[0].parent.as_ref().unwrap().name, "express");
    }

    #[test]
    fn test_parse_deeply_nested_overrides() {
        // Test 3-level nesting: express > body-parser > debug
        let pkg = json!({
            "name": "test",
            "overrides": {
                "express": {
                    "body-parser": {
                        "debug": "3.0.0"
                    }
                }
            }
        });

        let overrides = Overrides::parse(pkg).unwrap();
        assert_eq!(overrides.rules.len(), 1);
        assert_eq!(overrides.rules[0].name, "debug");
        assert_eq!(overrides.rules[0].target_spec, "3.0.0");

        // Check parent chain: debug -> body-parser -> express
        let parent1 = overrides.rules[0].parent.as_ref().unwrap();
        assert_eq!(parent1.name, "body-parser");

        let parent2 = parent1.parent.as_ref().unwrap();
        assert_eq!(parent2.name, "express");
        assert!(parent2.parent.is_none());
    }

    #[test]
    fn test_parse_multiple_workspace_overrides() {
        // Simulate workspace scenario with different overrides per workspace
        let pkg = json!({
            "name": "monorepo",
            "workspaces": ["packages/*"],
            "overrides": {
                "workspace-a": {
                    "lodash": "3.0.0"
                },
                "workspace-b": {
                    "lodash": "4.0.0"
                }
            }
        });

        let overrides = Overrides::parse(pkg).unwrap();
        assert_eq!(overrides.rules.len(), 2);

        // Find rules by parent name
        let rule_a = overrides
            .rules
            .iter()
            .find(|r| r.parent.as_ref().map(|p| p.name.as_str()) == Some("workspace-a"))
            .unwrap();
        assert_eq!(rule_a.name, "lodash");
        assert_eq!(rule_a.target_spec, "3.0.0");

        let rule_b = overrides
            .rules
            .iter()
            .find(|r| r.parent.as_ref().map(|p| p.name.as_str()) == Some("workspace-b"))
            .unwrap();
        assert_eq!(rule_b.name, "lodash");
        assert_eq!(rule_b.target_spec, "4.0.0");
    }

    #[test]
    fn test_parse_self_reference_override() {
        // Test "." syntax for overriding the parent package itself
        let pkg = json!({
            "name": "test",
            "overrides": {
                "express@^4.0.0": {
                    ".": "4.18.0"
                }
            }
        });

        let overrides = Overrides::parse(pkg).unwrap();
        assert_eq!(overrides.rules.len(), 1);
        assert_eq!(overrides.rules[0].name, "express");
        assert_eq!(overrides.rules[0].spec, "^4.0.0");
        assert_eq!(overrides.rules[0].target_spec, "4.18.0");
    }

    #[test]
    fn test_parse_resolutions_yarn_format() {
        // Test yarn's resolutions format
        let pkg = json!({
            "name": "test",
            "resolutions": {
                "lodash": "4.17.21"
            }
        });

        let overrides = Overrides::parse(pkg).unwrap();
        assert_eq!(overrides.rules.len(), 1);
        assert_eq!(overrides.rules[0].name, "lodash");
        assert_eq!(overrides.rules[0].target_spec, "4.17.21");
    }

    #[test]
    fn test_parse_merged_overrides_and_resolutions() {
        // Test that both overrides and resolutions are merged
        let pkg = json!({
            "name": "test",
            "overrides": {
                "lodash": "4.17.21"
            },
            "resolutions": {
                "debug": "4.0.0"
            }
        });

        let overrides = Overrides::parse(pkg).unwrap();
        assert_eq!(overrides.rules.len(), 2);

        let lodash_rule = overrides.rules.iter().find(|r| r.name == "lodash").unwrap();
        assert_eq!(lodash_rule.target_spec, "4.17.21");

        let debug_rule = overrides.rules.iter().find(|r| r.name == "debug").unwrap();
        assert_eq!(debug_rule.target_spec, "4.0.0");
    }

    #[test]
    fn test_parse_reference_from_dev_dependencies() {
        // Test $dep_name reference from devDependencies
        let pkg = json!({
            "name": "test",
            "devDependencies": {
                "typescript": "^5.0.0"
            },
            "overrides": {
                "typescript": "$typescript"
            }
        });

        let overrides = Overrides::parse(pkg).unwrap();
        assert_eq!(overrides.rules.len(), 1);
        assert_eq!(overrides.rules[0].target_spec, "^5.0.0");
    }

    #[test]
    fn test_parse_empty_overrides() {
        let pkg = json!({
            "name": "test"
        });

        let overrides = Overrides::parse(pkg).unwrap();
        assert!(overrides.rules.is_empty());
    }

    #[test]
    fn test_parse_scoped_package_override() {
        let pkg = json!({
            "name": "test",
            "overrides": {
                "@babel/core": "7.20.0"
            }
        });

        let overrides = Overrides::parse(pkg).unwrap();
        assert_eq!(overrides.rules.len(), 1);
        assert_eq!(overrides.rules[0].name, "@babel/core");
        assert_eq!(overrides.rules[0].target_spec, "7.20.0");
    }

    #[test]
    fn test_parse_scoped_package_with_version() {
        let pkg = json!({
            "name": "test",
            "overrides": {
                "@babel/core@^7.0.0": "7.20.0"
            }
        });

        let overrides = Overrides::parse(pkg).unwrap();
        assert_eq!(overrides.rules.len(), 1);
        assert_eq!(overrides.rules[0].name, "@babel/core");
        assert_eq!(overrides.rules[0].spec, "^7.0.0");
        assert_eq!(overrides.rules[0].target_spec, "7.20.0");
    }
}
