//! Spec transform pipeline.
//!
//! Protocols that rewrite dependency specifiers before graph building
//! implement [`SpecTransform`] and register in [`TRANSFORMS`].
//!
//! Adding a new transform protocol:
//! 1. Create a unit struct (e.g., `WorkspaceTransform`)
//! 2. Implement [`SpecTransform`] for it
//! 3. Add `&WorkspaceTransform` to [`TRANSFORMS`]

use std::collections::HashMap;

use super::Catalogs;
use crate::model::package_json::{DepsView, PackageJson};

// ---------------------------------------------------------------------------
// Trait
// ---------------------------------------------------------------------------

/// Result of a spec transformation.
pub enum TransformResult {
    /// Spec was rewritten to a new value.
    Rewritten(String),
    /// Spec is not handled by this transform.
    Unchanged,
}

/// Context available to all spec transforms.
pub struct TransformContext {
    /// Catalog definitions from `.utoo.toml`.
    pub catalogs: Catalogs,
    // Future: pub workspaces: HashMap<String, String>,
}

impl TransformContext {
    /// Returns true if there are no transform sources loaded,
    /// meaning `transform_specs` would be a no-op.
    pub fn is_empty(&self) -> bool {
        self.catalogs.is_empty()
    }
}

/// A pre-processing transform that rewrites dependency specifiers
/// before the dependency graph is built.
///
/// Each implementor handles a single protocol prefix (e.g., `catalog:`).
pub trait SpecTransform: Sync {
    /// The protocol prefix this transform handles (e.g., `"catalog:"`).
    fn prefix(&self) -> &'static str;

    /// Transform a spec. `rest` is the part after the prefix.
    ///
    /// Return [`TransformResult::Rewritten`] with the resolved spec,
    /// or [`TransformResult::Unchanged`] if this transform doesn't apply.
    fn transform(&self, pkg_name: &str, rest: &str, ctx: &TransformContext) -> TransformResult;
}

// ---------------------------------------------------------------------------
// Registry — add new transforms here
// ---------------------------------------------------------------------------

/// All registered spec transforms, checked in order.
///
/// To add a new transform protocol, append `&YourTransform` here.
pub static TRANSFORMS: &[&dyn SpecTransform] = &[
    &CatalogTransform,
    // Future: &WorkspaceTransform,
];

// ---------------------------------------------------------------------------
// Catalog transform
// ---------------------------------------------------------------------------

/// Resolves `catalog:` specifiers using definitions from `.utoo.toml`.
///
/// - `"catalog:"` or `"catalog:default"` → default catalog (key `""`)
/// - `"catalog:<name>"` → named catalog
struct CatalogTransform;

impl SpecTransform for CatalogTransform {
    fn prefix(&self) -> &'static str {
        "catalog:"
    }

    fn transform(&self, pkg_name: &str, rest: &str, ctx: &TransformContext) -> TransformResult {
        let catalog_key = if rest.is_empty() || rest == "default" {
            ""
        } else {
            rest
        };
        let display_name = if catalog_key.is_empty() {
            "default"
        } else {
            catalog_key
        };

        if let Some(catalog) = ctx.catalogs.get(catalog_key) {
            if let Some(resolved) = catalog.get(pkg_name) {
                tracing::debug!(
                    "catalog: resolved {}@catalog:{} -> {}",
                    pkg_name,
                    display_name,
                    resolved
                );
                return TransformResult::Rewritten(resolved.clone());
            }
            tracing::warn!(
                "catalog: package '{}' not found in catalog '{}'",
                pkg_name,
                display_name
            );
        } else {
            tracing::warn!(
                "catalog: catalog '{}' not found (referenced by {})",
                display_name,
                pkg_name
            );
        }

        TransformResult::Unchanged
    }
}

// ---------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------

/// Apply all registered transforms to a single dependency map.
pub fn transform_specs(deps: &mut HashMap<String, String>, ctx: &TransformContext) {
    if ctx.is_empty() {
        return;
    }
    for (pkg_name, spec) in deps.iter_mut() {
        // Fast path: most specs are plain semver ranges without a protocol prefix.
        if !spec.contains(':') {
            continue;
        }
        for t in TRANSFORMS {
            if let Some(rest) = spec.strip_prefix(t.prefix()) {
                if let TransformResult::Rewritten(new_spec) = t.transform(pkg_name, rest, ctx) {
                    *spec = new_spec;
                }
                break;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Convenience trait for types with dependency maps
// ---------------------------------------------------------------------------

/// Apply all spec transforms to all dependency maps on a type.
pub trait TransformSpecs {
    fn transform_specs(&mut self, ctx: &TransformContext);
}

impl TransformSpecs for PackageJson {
    fn transform_specs(&mut self, ctx: &TransformContext) {
        for deps in [
            &mut self.dependencies,
            &mut self.dev_dependencies,
            &mut self.peer_dependencies,
            &mut self.optional_dependencies,
        ]
        .into_iter()
        .flatten()
        {
            transform_specs(deps, ctx);
        }
    }
}

impl TransformSpecs for DepsView {
    fn transform_specs(&mut self, ctx: &TransformContext) {
        for deps in [
            &mut self.dependencies,
            &mut self.dev_dependencies,
            &mut self.peer_dependencies,
            &mut self.optional_dependencies,
        ] {
            transform_specs(deps, ctx);
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_ctx(catalogs: Catalogs) -> TransformContext {
        TransformContext { catalogs }
    }

    #[test]
    fn test_catalog_transform_default() {
        let ctx = make_ctx(HashMap::from([(
            String::new(),
            HashMap::from([("lodash".to_string(), "^4.17.21".to_string())]),
        )]));

        let mut deps = HashMap::from([
            ("lodash".to_string(), "catalog:".to_string()),
            ("express".to_string(), "^4.18.0".to_string()),
        ]);

        transform_specs(&mut deps, &ctx);

        assert_eq!(deps["lodash"], "^4.17.21");
        assert_eq!(deps["express"], "^4.18.0");
    }

    #[test]
    fn test_catalog_transform_explicit_default() {
        let ctx = make_ctx(HashMap::from([(
            String::new(),
            HashMap::from([("typescript".to_string(), "^5.0.0".to_string())]),
        )]));

        let mut deps = HashMap::from([("typescript".to_string(), "catalog:default".to_string())]);

        transform_specs(&mut deps, &ctx);

        assert_eq!(deps["typescript"], "^5.0.0");
    }

    #[test]
    fn test_catalog_transform_named() {
        let ctx = make_ctx(HashMap::from([(
            "legacy".to_string(),
            HashMap::from([("express".to_string(), "^3.0.0".to_string())]),
        )]));

        let mut deps = HashMap::from([("express".to_string(), "catalog:legacy".to_string())]);

        transform_specs(&mut deps, &ctx);

        assert_eq!(deps["express"], "^3.0.0");
    }

    #[test]
    fn test_catalog_transform_missing_catalog() {
        let ctx = make_ctx(HashMap::new());
        let mut deps = HashMap::from([("lodash".to_string(), "catalog:".to_string())]);

        transform_specs(&mut deps, &ctx);

        assert_eq!(deps["lodash"], "catalog:");
    }

    #[test]
    fn test_catalog_transform_missing_package() {
        let ctx = make_ctx(HashMap::from([(
            String::new(),
            HashMap::from([("react".to_string(), "^18.0.0".to_string())]),
        )]));

        let mut deps = HashMap::from([("lodash".to_string(), "catalog:".to_string())]);

        transform_specs(&mut deps, &ctx);

        assert_eq!(deps["lodash"], "catalog:");
    }

    #[test]
    fn test_catalog_transform_mixed_default_and_named() {
        let ctx = make_ctx(HashMap::from([
            (
                String::new(),
                HashMap::from([("debug".to_string(), "^4.3.4".to_string())]),
            ),
            (
                "legacy".to_string(),
                HashMap::from([("debug".to_string(), "^3.2.7".to_string())]),
            ),
        ]));

        let mut deps_default = HashMap::from([("debug".to_string(), "catalog:".to_string())]);
        let mut deps_named = HashMap::from([("debug".to_string(), "catalog:legacy".to_string())]);

        transform_specs(&mut deps_default, &ctx);
        transform_specs(&mut deps_named, &ctx);

        assert_eq!(deps_default["debug"], "^4.3.4");
        assert_eq!(deps_named["debug"], "^3.2.7");
    }

    #[test]
    fn test_transform_specs_all_dep_types() {
        let ctx = make_ctx(HashMap::from([(
            String::new(),
            HashMap::from([
                ("lodash".to_string(), "^4.17.21".to_string()),
                ("vitest".to_string(), "^1.0.0".to_string()),
                ("react".to_string(), "^18.0.0".to_string()),
                ("zod".to_string(), "^3.0.0".to_string()),
            ]),
        )]));

        let mut pkg = PackageJson {
            name: "test".to_string(),
            version: "1.0.0".to_string(),
            dependencies: Some(HashMap::from([(
                "lodash".to_string(),
                "catalog:".to_string(),
            )])),
            dev_dependencies: Some(HashMap::from([(
                "vitest".to_string(),
                "catalog:".to_string(),
            )])),
            peer_dependencies: Some(HashMap::from([(
                "react".to_string(),
                "catalog:".to_string(),
            )])),
            optional_dependencies: Some(HashMap::from([(
                "zod".to_string(),
                "catalog:".to_string(),
            )])),
            ..Default::default()
        };

        pkg.transform_specs(&ctx);

        assert_eq!(pkg.dependencies.as_ref().unwrap()["lodash"], "^4.17.21");
        assert_eq!(pkg.dev_dependencies.as_ref().unwrap()["vitest"], "^1.0.0");
        assert_eq!(pkg.peer_dependencies.as_ref().unwrap()["react"], "^18.0.0");
        assert_eq!(pkg.optional_dependencies.as_ref().unwrap()["zod"], "^3.0.0");
    }

    #[test]
    fn test_transform_specs_empty_ctx_is_noop() {
        let ctx = make_ctx(HashMap::new());
        let mut pkg = PackageJson {
            name: "test".to_string(),
            version: "1.0.0".to_string(),
            dependencies: Some(HashMap::from([(
                "lodash".to_string(),
                "catalog:".to_string(),
            )])),
            ..Default::default()
        };

        pkg.transform_specs(&ctx);

        assert_eq!(pkg.dependencies.as_ref().unwrap()["lodash"], "catalog:");
    }

    #[test]
    fn test_non_catalog_specs_untouched() {
        let ctx = make_ctx(HashMap::from([(
            String::new(),
            HashMap::from([("lodash".to_string(), "^4.17.21".to_string())]),
        )]));

        let mut deps = HashMap::from([
            ("express".to_string(), "^4.18.0".to_string()),
            ("debug".to_string(), "workspace:*".to_string()),
            ("local".to_string(), "file:../lib".to_string()),
        ]);

        transform_specs(&mut deps, &ctx);

        assert_eq!(deps["express"], "^4.18.0");
        assert_eq!(deps["debug"], "workspace:*");
        assert_eq!(deps["local"], "file:../lib");
    }
}
