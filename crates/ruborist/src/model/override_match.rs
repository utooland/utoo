//! Override-rule matching against the dependency graph.
//!
//! The parsing half of npm `overrides` / yarn `resolutions` lives in
//! [`super::override_rule`]; this module owns the *matching* half — walking a
//! node's physical parent chain and deciding whether a rule applies. Kept out
//! of `graph.rs` so the graph file stays focused on the data structure.

use petgraph::graph::NodeIndex;

use super::graph::DependencyGraph;
use super::override_rule::OverrideRule;
use crate::resolver::semver::matches;

impl DependencyGraph {
    /// Collect the physical parent chain from a node up to root (excluding root).
    ///
    /// Returns the chain in order from outermost to innermost.
    /// INCLUDES the `from` node itself (as the innermost), plus all its ancestors.
    ///
    /// Example: root -> express -> body-parser
    /// If `from` = body-parser, returns [(express, version), (body-parser, version)]
    ///
    /// This is used for override matching where the rule specifies:
    /// "express": { "body-parser": { "debug": "4.0.0" } }
    /// meaning debug should be overridden when its parent is body-parser AND
    /// body-parser's parent is express.
    fn collect_parent_chain(&self, from: NodeIndex) -> Vec<(String, String)> {
        let mut chain = Vec::new();
        let mut current = from;

        // First, add the current node itself (if not root)
        let from_node = &self.graph[from];
        if !from_node.is_root() {
            chain.push((from_node.name.clone(), from_node.version.clone()));
        }

        // Then collect all physical parents up to root (excluding root)
        while let Some(parent) = self.get_physical_parent(current) {
            let parent_node = &self.graph[parent];
            if !parent_node.is_root() {
                chain.push((parent_node.name.clone(), parent_node.version.clone()));
            }
            current = parent;
        }

        chain.reverse();
        chain
    }

    /// Check if an override rule applies.
    ///
    /// - `resolved_version = None`: Only check unconditional overrides (spec == "*")
    /// - `resolved_version = Some(v)`: Check both unconditional and conditional overrides
    ///
    /// For conditional overrides like `immer@^9: 8.0.0`, the resolved version
    /// (e.g., "9.0.21") is matched against the rule spec ("^9").
    pub fn check_override(
        &self,
        from: NodeIndex,
        name: &str,
        resolved_version: Option<&str>,
    ) -> Option<String> {
        // Fast path: skip if name is not in override_names
        if !self.override_names.contains(name) {
            return None;
        }

        let overrides = self.overrides.as_ref()?;
        let parent_chain = self.collect_parent_chain(from);

        for rule in &overrides.rules {
            if rule.name != name {
                continue;
            }

            // Check spec matching
            let spec_matches = if rule.spec == "*" {
                // Unconditional override: always matches
                true
            } else if let Some(version) = resolved_version {
                // Conditional override: check if resolved version matches rule spec
                matches(&rule.spec, version)
            } else {
                // No resolved version provided, skip conditional overrides
                false
            };

            if !spec_matches {
                continue;
            }

            // Check if parent chain matches
            if self.matches_parent_chain_for_rule(rule, &parent_chain) {
                tracing::debug!(
                    "Override matched: {}@{} => {} (version: {:?}, from: {:?})",
                    name,
                    rule.spec,
                    rule.target_spec,
                    resolved_version,
                    from
                );
                return Some(rule.target_spec.clone());
            }
        }

        None
    }

    /// Check if a parent chain matches an override rule's parent condition.
    ///
    /// The rule's parent chain is from inner to outer: debug.parent = body-parser,
    /// body-parser.parent = express.
    ///
    /// The parent_chain is from outer to inner: [express, body-parser].
    ///
    /// We need to match starting from the innermost (end of parent_chain) and
    /// work outward, checking that each rule parent matches.
    fn matches_parent_chain_for_rule(
        &self,
        rule: &OverrideRule,
        parent_chain: &[(String, String)],
    ) -> bool {
        // If no parent requirement, always matches
        if rule.parent.is_none() {
            return true;
        }

        // Start from the end of parent_chain (innermost parent)
        let mut current_rule = rule.parent.as_ref();
        let mut chain_idx = parent_chain.len();

        while chain_idx > 0 {
            chain_idx -= 1;
            let (parent_name, parent_version) = &parent_chain[chain_idx];

            if let Some(rule_ref) = current_rule {
                if parent_name == &rule_ref.name {
                    // Check if version matches
                    let version_matches =
                        rule_ref.spec == "*" || matches(&rule_ref.spec, parent_version);

                    if version_matches {
                        // Move to next (outer) parent requirement
                        current_rule = rule_ref.parent.as_ref();
                        if current_rule.is_none() {
                            // All parent requirements matched
                            return true;
                        }
                        continue;
                    }
                }
            } else {
                // All rule requirements already matched
                return true;
            }
        }

        // If current_rule is still Some, not all parent requirements were matched
        current_rule.is_none()
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use serde_json::json;

    use super::*;
    use crate::model::graph::{FindResult, PackageNode};
    use crate::model::manifest::CoreVersionManifest;
    use crate::model::package_json::PackageJson;

    fn create_pkg(name: &str, version: &str) -> PackageJson {
        PackageJson::new(name, version)
    }

    fn create_version_manifest(name: &str, version: &str) -> Arc<CoreVersionManifest> {
        Arc::new(CoreVersionManifest {
            name: name.to_string(),
            version: version.to_string(),
            ..Default::default()
        })
    }

    #[test]
    fn test_workspace_override() {
        // Create root package.json with workspaces and overrides
        let pkg_value = json!({
            "name": "root",
            "version": "1.0.0",
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
        let pkg = PackageJson::from_value(&pkg_value).unwrap();
        let mut graph = DependencyGraph::from_package_json(PathBuf::from("."), pkg);

        // Add workspace-a under root
        let ws_a_pkg = create_pkg("workspace-a", "1.0.0");
        let ws_a = PackageNode::workspace_from_package_json(
            PathBuf::from("packages/workspace-a"),
            ws_a_pkg,
        );
        let ws_a_idx = graph.add_node(ws_a);
        graph.add_physical_edge(graph.root_index, ws_a_idx);

        // Add workspace-b under root
        let ws_b_pkg = create_pkg("workspace-b", "1.0.0");
        let ws_b = PackageNode::workspace_from_package_json(
            PathBuf::from("packages/workspace-b"),
            ws_b_pkg,
        );
        let ws_b_idx = graph.add_node(ws_b);
        graph.add_physical_edge(graph.root_index, ws_b_idx);

        // Check unconditional override for workspace-a's lodash dependency
        let override_a = graph.check_override(ws_a_idx, "lodash", None);
        assert_eq!(override_a, Some("3.0.0".to_string()));

        // Check unconditional override for workspace-b's lodash dependency
        let override_b = graph.check_override(ws_b_idx, "lodash", None);
        assert_eq!(override_b, Some("4.0.0".to_string()));

        // Root's lodash should not have override (no matching parent context)
        let override_root = graph.check_override(graph.root_index, "lodash", None);
        assert_eq!(override_root, None);
    }

    #[test]
    fn test_simple_override() {
        // Simple global override: all lodash -> 4.17.21
        let pkg_value = json!({
            "name": "root",
            "version": "1.0.0",
            "overrides": {
                "lodash": "4.17.21"
            }
        });
        let pkg = PackageJson::from_value(&pkg_value).unwrap();
        let graph = DependencyGraph::from_package_json(PathBuf::from("."), pkg);

        // Unconditional override should apply
        let override_result = graph.check_override(graph.root_index, "lodash", None);
        assert_eq!(override_result, Some("4.17.21".to_string()));
    }

    #[test]
    fn test_versioned_override() {
        // Override only specific version range
        let pkg_value = json!({
            "name": "root",
            "version": "1.0.0",
            "overrides": {
                "lodash@^3.0.0": "4.17.21"
            }
        });
        let pkg = PackageJson::from_value(&pkg_value).unwrap();
        let graph = DependencyGraph::from_package_json(PathBuf::from("."), pkg);

        // Should apply when resolved version matches ^3.0.0
        let override_result = graph.check_override(graph.root_index, "lodash", Some("3.10.0"));
        assert_eq!(override_result, Some("4.17.21".to_string()));

        // Should NOT apply when resolved version doesn't match
        let override_result2 = graph.check_override(graph.root_index, "lodash", Some("4.0.0"));
        assert_eq!(override_result2, None);
    }

    #[test]
    fn test_nested_override_chain() {
        // Override debug only under express
        let pkg_value = json!({
            "name": "root",
            "version": "1.0.0",
            "overrides": {
                "express": {
                    "debug": "4.0.0"
                }
            }
        });
        let pkg = PackageJson::from_value(&pkg_value).unwrap();
        let mut graph = DependencyGraph::from_package_json(PathBuf::from("."), pkg);

        // Add express under root
        let express = PackageNode::from_version_manifest(
            "express".to_string(),
            PathBuf::from("node_modules/express"),
            create_version_manifest("express", "4.18.0"),
        );
        let express_idx = graph.add_node(express);
        graph.add_physical_edge(graph.root_index, express_idx);

        // Debug under express should be overridden (unconditional)
        let override_result = graph.check_override(express_idx, "debug", None);
        assert_eq!(override_result, Some("4.0.0".to_string()));

        // Debug directly under root should NOT be overridden
        let override_root = graph.check_override(graph.root_index, "debug", None);
        assert_eq!(override_root, None);
    }

    #[test]
    fn test_deeply_nested_override() {
        // Override only under express > body-parser
        let pkg_value = json!({
            "name": "root",
            "version": "1.0.0",
            "overrides": {
                "express": {
                    "body-parser": {
                        "debug": "4.0.0"
                    }
                }
            }
        });
        let pkg = PackageJson::from_value(&pkg_value).unwrap();
        let mut graph = DependencyGraph::from_package_json(PathBuf::from("."), pkg);

        // Build: root -> express -> body-parser
        let express = PackageNode::from_version_manifest(
            "express".to_string(),
            PathBuf::from("node_modules/express"),
            create_version_manifest("express", "4.18.0"),
        );
        let express_idx = graph.add_node(express);
        graph.add_physical_edge(graph.root_index, express_idx);

        let body_parser = PackageNode::from_version_manifest(
            "body-parser".to_string(),
            PathBuf::from("node_modules/express/node_modules/body-parser"),
            create_version_manifest("body-parser", "1.20.0"),
        );
        let body_parser_idx = graph.add_node(body_parser);
        graph.add_physical_edge(express_idx, body_parser_idx);

        // Debug under body-parser (which is under express) should be overridden (unconditional)
        let override_result = graph.check_override(body_parser_idx, "debug", None);
        assert_eq!(override_result, Some("4.0.0".to_string()));

        // Debug directly under express should NOT be overridden
        let override_express = graph.check_override(express_idx, "debug", None);
        assert_eq!(override_express, None);

        // Debug under root should NOT be overridden
        let override_root = graph.check_override(graph.root_index, "debug", None);
        assert_eq!(override_root, None);
    }

    #[test]
    fn test_override_with_find_compatible_node() {
        // Test that override affects find_compatible_node result
        let pkg_value = json!({
            "name": "root",
            "version": "1.0.0",
            "overrides": {
                "lodash": "4.17.21"
            }
        });
        let pkg = PackageJson::from_value(&pkg_value).unwrap();
        let mut graph = DependencyGraph::from_package_json(PathBuf::from("."), pkg);

        // Add lodash@4.17.21 under root (matching the override)
        let lodash = PackageNode::from_version_manifest(
            "lodash".to_string(),
            PathBuf::from("node_modules/lodash"),
            create_version_manifest("lodash", "4.17.21"),
        );
        let lodash_idx = graph.add_node(lodash);
        graph.add_physical_edge(graph.root_index, lodash_idx);

        // Even though someone requests ^3.0.0, the override converts it to 4.17.21
        // So it should find and reuse the existing lodash@4.17.21
        let result = graph.find_compatible_node(graph.root_index, "lodash", "^3.0.0");
        assert_eq!(result, FindResult::Reuse(lodash_idx));
    }

    #[test]
    fn test_no_override_when_name_not_matched() {
        let pkg_value = json!({
            "name": "root",
            "version": "1.0.0",
            "overrides": {
                "lodash": "4.17.21"
            }
        });
        let pkg = PackageJson::from_value(&pkg_value).unwrap();
        let graph = DependencyGraph::from_package_json(PathBuf::from("."), pkg);

        // Other packages should not be affected
        let override_result = graph.check_override(graph.root_index, "underscore", None);
        assert_eq!(override_result, None);
    }

    #[test]
    fn test_scoped_package_override() {
        let pkg_value = json!({
            "name": "root",
            "version": "1.0.0",
            "overrides": {
                "@babel/core": "7.20.0"
            }
        });
        let pkg = PackageJson::from_value(&pkg_value).unwrap();
        let graph = DependencyGraph::from_package_json(PathBuf::from("."), pkg);

        // Unconditional override for scoped package
        let override_result = graph.check_override(graph.root_index, "@babel/core", None);
        assert_eq!(override_result, Some("7.20.0".to_string()));
    }

    #[test]
    fn test_reference_override() {
        // Test $dep_name reference
        let pkg_value = json!({
            "name": "root",
            "version": "1.0.0",
            "dependencies": {
                "lodash": "^4.17.0"
            },
            "overrides": {
                "lodash": "$lodash"
            }
        });
        let pkg = PackageJson::from_value(&pkg_value).unwrap();
        let graph = DependencyGraph::from_package_json(PathBuf::from("."), pkg);

        // Override should resolve to the version in dependencies (unconditional)
        let override_result = graph.check_override(graph.root_index, "lodash", None);
        assert_eq!(override_result, Some("^4.17.0".to_string()));
    }

    #[test]
    fn test_conditional_override_with_resolved_version() {
        // Conditional override: only apply when resolved version matches ^9
        let pkg_value = json!({
            "name": "root",
            "version": "1.0.0",
            "overrides": {
                "immer@^9": "8.0.0"
            }
        });
        let pkg = PackageJson::from_value(&pkg_value).unwrap();
        let graph = DependencyGraph::from_package_json(PathBuf::from("."), pkg);

        // Should apply when resolved version is 9.0.21 (matches ^9)
        let override_result = graph.check_override(graph.root_index, "immer", Some("9.0.21"));
        assert_eq!(override_result, Some("8.0.0".to_string()));

        // Should NOT apply when resolved version is 8.0.0 (doesn't match ^9)
        let override_result2 = graph.check_override(graph.root_index, "immer", Some("8.0.0"));
        assert_eq!(override_result2, None);

        // Should NOT match unconditional (no version provided)
        let unconditional = graph.check_override(graph.root_index, "immer", None);
        assert_eq!(unconditional, None);
    }
}
