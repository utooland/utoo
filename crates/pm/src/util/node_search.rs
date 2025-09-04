use anyhow::Result;
use std::sync::Arc;

use crate::model::node::Node;

pub async fn get_node_from_root_by_path(root: &Arc<Node>, pkg_path: &str) -> Result<Arc<Node>> {
    // Split the path by node_modules to get the package hierarchy
    let path_parts: Vec<&str> = pkg_path
        .trim_end_matches('/') // Remove trailing slash
        .split("node_modules/")
        .filter(|s| !s.is_empty())
        .map(|s| s.trim_end_matches('/')) // Remove trailing slash from each part
        .collect();

    // Start from the root node
    let mut current_node = root.clone();

    // Navigate through the node_modules hierarchy
    for part in path_parts {
        let mut found = false;
        let next_node = {
            let children = current_node.children.read().unwrap();
            let mut result = None;

            // Look for the next node in the hierarchy
            for child in children.iter() {
                if child.name == part {
                    result = Some(child.clone());
                    found = true;
                    break;
                }
            }
            result
        };

        if !found {
            return Err(anyhow::anyhow!(
                "Could not find package at path {}",
                pkg_path
            ));
        }

        current_node = next_node.unwrap();
    }
    Ok(current_node)
}


#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::PathBuf;

    #[tokio::test]
    async fn test_get_node_from_root_by_path() {
        let root = Node::new_root(
            "test-package".to_string(),
            PathBuf::from("."),
            json!({
                "name": "test-package",
                "version": "1.0.0"
            }),
        );

        let child1 = Node::new(
            "lodash".to_string(),
            PathBuf::from("node_modules/lodash"),
            json!({
                "name": "lodash",
                "version": "4.17.20"
            }),
        );

        let child2 = Node::new(
            "express".to_string(),
            PathBuf::from("node_modules/express"),
            json!({
                "name": "express",
                "version": "4.17.1"
            }),
        );

        {
            let mut children = root.children.write().unwrap();
            children.push(child1.clone());
            children.push(child2.clone());
        }

        let result = get_node_from_root_by_path(&root, "node_modules/lodash").await;
        assert!(result.is_ok());
        let node = result.unwrap();
        assert_eq!(node.name, "lodash");
        assert_eq!(node.version, "4.17.20");

        let result = get_node_from_root_by_path(&root, "node_modules/express/").await;
        assert!(result.is_ok());
        let node = result.unwrap();
        assert_eq!(node.name, "express");
        assert_eq!(node.version, "4.17.1");

        let result = get_node_from_root_by_path(&root, "node_modules/non-existent").await;
        assert!(result.is_err());

        let result = get_node_from_root_by_path(&root, "").await;
        assert!(result.is_ok());
        let node = result.unwrap();
        assert_eq!(node.name, "test-package");
    }

}