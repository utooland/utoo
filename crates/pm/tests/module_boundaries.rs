//! Compile-time source architecture checks, excluding test-only modules.
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use syn::visit::{self, Visit};
use syn::{ItemMod, ItemUse, UseTree};

fn source_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files.extend(source_files(&path));
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
    files
}

#[derive(Default)]
struct References {
    paths: Vec<String>,
    aliases: HashMap<String, String>,
}

impl References {
    fn import(&mut self, prefix: &str, tree: &UseTree) {
        match tree {
            UseTree::Path(path) => self.import(&format!("{prefix}{}::", path.ident), &path.tree),
            UseTree::Group(group) => {
                for tree in &group.items {
                    self.import(prefix, tree);
                }
            }
            UseTree::Name(name) => {
                let (path, alias) = if name.ident == "self" {
                    let path = prefix.trim_end_matches("::").to_owned();
                    let alias = path.rsplit("::").next().unwrap().to_owned();
                    (path, alias)
                } else {
                    (format!("{prefix}{}", name.ident), name.ident.to_string())
                };
                self.paths.push(path.clone());
                self.aliases.insert(alias, path);
            }
            UseTree::Rename(rename) => {
                let path = format!("{prefix}{}", rename.ident);
                self.paths.push(path.clone());
                self.aliases.insert(rename.rename.to_string(), path);
            }
            UseTree::Glob(_) => self.paths.push(format!("{prefix}*")),
        }
    }
}

impl<'ast> Visit<'ast> for References {
    fn visit_item_mod(&mut self, module: &'ast ItemMod) {
        if module.attrs.iter().any(|attribute| {
            attribute.path().is_ident("cfg")
                && attribute
                    .parse_args::<syn::Path>()
                    .is_ok_and(|path| path.is_ident("test"))
        }) {
            return;
        }
        visit::visit_item_mod(self, module);
    }

    fn visit_item_use(&mut self, item: &'ast ItemUse) {
        self.import("", &item.tree);
    }

    fn visit_path(&mut self, path: &'ast syn::Path) {
        self.paths.push(
            path.segments
                .iter()
                .map(|segment| segment.ident.to_string())
                .collect::<Vec<_>>()
                .join("::"),
        );
        visit::visit_path(self, path);
    }
}

fn violations(source: &str, forbidden: &[&str]) -> Vec<String> {
    let mut references = References::default();
    references.visit_file(&syn::parse_file(source).expect("valid Rust source"));
    references
        .paths
        .into_iter()
        .filter_map(|path| {
            let (first, rest) = path.split_once("::").unwrap_or((&path, ""));
            let path = references.aliases.get(first).map_or_else(
                || path.clone(),
                |alias| {
                    if rest.is_empty() {
                        alias.clone()
                    } else {
                        format!("{alias}::{rest}")
                    }
                },
            );
            forbidden
                .iter()
                .any(|prefix| path == *prefix || path.starts_with(&format!("{prefix}::")))
                .then_some(path)
        })
        .collect()
}

#[test]
fn pm_and_ruborist_keep_their_dependency_directions() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut failures = Vec::new();
    for name in ["pm", "utoo-wasm", "ruborist"] {
        for file in source_files(&crates.join(name).join("src")) {
            let relative = file
                .strip_prefix(crates)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            let mut forbidden = Vec::new();
            if name != "ruborist" {
                forbidden.extend([
                    "utoo_ruborist::resolver",
                    "utoo_ruborist::model",
                    "utoo_ruborist::traits",
                ]);
            }
            if relative.starts_with("pm/src/service/") {
                forbidden.extend([
                    "crate::cmd",
                    "crate::cli",
                    "std::process::exit",
                    "std::env::current_dir",
                    "std::env::set_current_dir",
                    "std::env::args",
                    "std::env::args_os",
                    "std::env::*",
                ]);
            }
            if relative.starts_with("pm/src/model/") {
                forbidden.push("crate::service");
            }
            if relative.starts_with("ruborist/src/sources/")
                || relative.starts_with("ruborist/src/service/registry/")
            {
                forbidden.extend([
                    "crate::model::graph",
                    "crate::model::node",
                    "crate::model::edge",
                    "crate::resolver::builder",
                    "crate::resolver::demand",
                    "crate::resolver::placement",
                ]);
            }
            for path in violations(&std::fs::read_to_string(&file).unwrap(), &forbidden) {
                failures.push(format!("{relative} imports/calls {path}"));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "module boundary violations:\n{}",
        failures.join("\n")
    );
}

#[test]
fn grouped_and_aliased_imports_are_checked_but_docs_and_tests_are_not() {
    let source = r#"
        use std::{env as environment, process};
        /// Example mentions std::process::exit without calling it.
        fn operation() { environment::set_current_dir(".").unwrap(); process::exit(1); }
        #[cfg(test)] mod tests { fn test() { std::env::current_dir().unwrap(); } }
    "#;
    assert_eq!(
        violations(
            source,
            &[
                "std::env::set_current_dir",
                "std::process::exit",
                "std::env::current_dir"
            ]
        ),
        ["std::env::set_current_dir", "std::process::exit"]
    );
}
