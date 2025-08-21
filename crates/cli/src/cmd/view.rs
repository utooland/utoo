use crate::helper::package::parse_package_spec;
use crate::util::logger::log_verbose;
use crate::util::registry::{resolve, get_package_info};
use anyhow::{anyhow, Result};
use chrono::{TimeZone, Utc};
use owo_colors::OwoColorize;
use serde_json::Value;
use term_size;

/// View package information from registry, similar to npm view
pub async fn view(package_spec: &str) -> Result<()> {
    log_verbose(&format!("Viewing package: {}", package_spec));

    // Parse package specification
    let (name, version_spec) = parse_package_spec(package_spec);
    
    log_verbose(&format!("Resolved package: {} (spec: {})", name, version_spec));

    // Get complete package information (like npm view)
    let package_info = get_package_info(name)
        .await
        .map_err(|e| anyhow!("Failed to fetch package info for {}, reason: {}", package_spec, e))?;

    // Get the specific version manifest if a version was specified
    let resolved_package = resolve(name, version_spec).await?;
    let version_manifest = resolved_package.manifest;

    // Print package information in npm view format
    print_package_info(&package_info, name, &version_manifest)?;

    Ok(())
}

fn print_grid(items: Vec<String>) {
    let terminal_width = term_size::dimensions()
        .map(|(w, _)| w)
        .unwrap_or(80); // 默认80字符宽度
    log_verbose(&format!("Terminal size: {}", terminal_width));

    let max_len = items.iter()
        .map(|s| s.len())
        .max()
        .unwrap_or(1);
    log_verbose(&format!("Max item length: {}", max_len));

    for cols in [12, 6, 4, 3, 2, 1] {
        if (terminal_width / max_len) >= cols {
            let rows = (items.len() + cols - 1) / cols; // 向上取整
            let col_len = terminal_width / cols;
            log_verbose(&format!("Using {} columns, {} rows, column length {}", cols, rows, col_len));

            for row in 0..rows {
                let mut line = String::new();
                for col in 0..cols {
                    let index = col + row * cols;
                    if index < items.len() {
                        let item = items.get(index).unwrap();
                        line.push_str(&item);
                        if col < cols {
                            let spaces = " ".repeat(col_len - item.len());
                            line.push_str(&spaces);
                        }
                    }
                }
                println!("{}", line);
            }
            return;
        }
    }
}

/// Print package information in npm view style format
fn print_package_info(package_info: &Value, name: &str, version_manifest: &Value) -> Result<()> {
    // Get the latest version from package info
    let latest_version = package_info.get("dist-tags")
        .and_then(|tags| tags.get("latest"))
        .and_then(|v| v.as_str())
        .unwrap_or("latest");
    
    // Get the specific version if provided, otherwise use latest
    let target_version = version_manifest.get("version").and_then(|v| v.as_str()).unwrap_or(latest_version);

    log_verbose(&format!("Target version: {}", target_version));
    
    // Get the target manifest
    let target_manifest = version_manifest;
    
    // Print header line like npm view
    let description = target_manifest.get("description").and_then(|v| v.as_str()).unwrap_or("");
    
    // Try to get license from multiple sources
    let license = target_manifest.get("license")
        .and_then(|v| v.as_str())
        .or_else(|| package_info.get("license").and_then(|v| v.as_str()))
        .unwrap_or("UNLICENSED");
    
    // Count dependencies
    let deps_count = target_manifest.get("dependencies")
        .and_then(|v| v.as_object())
        .map(|obj| obj.len())
        .unwrap_or(0);
    
    // Count versions
    let versions_count = package_info.get("versions")
        .and_then(|v| v.as_object())
        .map(|obj| obj.len())
        .unwrap_or(0);
    
    let deps_str = if deps_count == 0 { "none" } else { &deps_count.to_string() };
    println!("\n{}@{} | {} | deps: {} | versions: {}", 
        name.bright_blue().bold(), 
        target_version.bright_green(), 
        license.yellow(), 
        deps_str.cyan(), 
        versions_count.magenta());
    
    if !description.is_empty() {
        println!("{}", description.white());
    }
    
    // Print homepage if available
    if let Some(homepage) = target_manifest.get("homepage").and_then(|v| v.as_str()) {
        println!("{}", homepage.blue().underline());
    }
    
    println!();
    
    // Print keywords
    if let Some(keywords) = target_manifest.get("keywords").and_then(|v| v.as_array()) {
        if !keywords.is_empty() {
            let keyword_str = keywords
                .iter()
                .filter_map(|k| k.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            println!("{} {}", "keywords:".bright_cyan(), keyword_str.white());
        }
    }
    
    // Print dist information
    if let Some(dist) = target_manifest.get("dist") {
        println!("\n{}", "dist".bright_yellow().bold());
        if let Some(tarball) = dist.get("tarball").and_then(|v| v.as_str()) {
            println!("{} {}", ".tarball:".cyan(), tarball.blue().underline());
        }
        if let Some(shasum) = dist.get("shasum").and_then(|v| v.as_str()) {
            println!("{} {}", ".shasum:".cyan(), shasum.green());
        }
        if let Some(integrity) = dist.get("integrity").and_then(|v| v.as_str()) {
            println!("{} {}", ".integrity:".cyan(), integrity.green());
        }
        if let Some(unpacked_size) = dist.get("unpackedSize").and_then(|v| v.as_u64()) {
            let size_mb = unpacked_size as f64 / 1024.0 / 1024.0;
            println!("{} {} MB", ".unpackedSize:".cyan(), format!("{:.1}", size_mb).yellow());
        }
    }
    
    // Print author information
    let author_source = target_manifest.get("author").or_else(|| package_info.get("author"));
    
    if let Some(author) = author_source.and_then(|v| v.as_object()) {
        if let Some(author_name) = author.get("name").and_then(|v| v.as_str()) {
            if let Some(author_email) = author.get("email").and_then(|v| v.as_str()) {
                println!("\n{} {} <{}>", "author:".bright_magenta(), author_name.white(), author_email.blue());
            } else {
                println!("\n{} {}", "author:".bright_magenta(), author_name.white());
            }
        }
    }
    
    // Print repository information
    let repo_source = target_manifest.get("repository").or_else(|| package_info.get("repository"));
    
    if let Some(repo) = repo_source.and_then(|v| v.as_object()) {
        if let Some(repo_type) = repo.get("type").and_then(|v| v.as_str()) {
            if let Some(repo_url) = repo.get("url").and_then(|v| v.as_str()) {
                println!("{} {}:{}", "repository:".bright_magenta(), repo_type.green(), repo_url.blue().underline());
            }
        }
    }
    
    // Print bugs information
    let bugs_source = target_manifest.get("bugs").or_else(|| package_info.get("bugs"));
    
    if let Some(bugs) = bugs_source.and_then(|v| v.as_object()) {
        if let Some(bugs_url) = bugs.get("url").and_then(|v| v.as_str()) {
            println!("{} {}", "bugs:".bright_magenta(), bugs_url.blue().underline());
        }
    }

    // Print dependencies
    if let Some(dependencies) = target_manifest.get("dependencies").and_then(|v|
        v.as_object()) {
        if !dependencies.is_empty() {
            println!("\n{} {}", "dependencies:".bright_yellow().bold(), dependencies.len().white());
            let show_count = 24;
            let show_deps = dependencies.iter()
                .take(show_count)
                .map(|(dep_name, dep_version)| if let Some(version_str) = dep_version.as_str() {
                    format!("{}: {}", dep_name.blue(), version_str)
                } else {
                    format!("{}: {}", dep_name.blue(), dep_version)
                })
                .collect::<Vec<_>>();
            print_grid(show_deps);
            if dependencies.len() > show_count {
                println!("(... and {} more.)", (dependencies.len() - show_count).to_string().white());
            }
        }
    }
    
    // Print maintainers
    if let Some(maintainers) = package_info.get("maintainers").and_then(|v| v.as_array()) {
        if !maintainers.is_empty() {
            println!("\n{}", "maintainers:".bright_yellow().bold());
            for maintainer in maintainers {
                if let Some(name) = maintainer.get("name").and_then(|v| v.as_str()) {
                    if let Some(email) = maintainer.get("email").and_then(|v| v.as_str()) {
                        println!("- {} <{}>", name.blue(), email.white());
                    } else {
                        println!("- {}", name.blue());
                    }
                }
            }
        }
    }
    
    // Print dist-tags
    if let Some(dist_tags) = package_info.get("dist-tags").and_then(|v| v.as_object()) {
        if !dist_tags.is_empty() {
            println!("\n{}", "dist-tags:".bright_yellow().bold());
            let tags = dist_tags.iter().map(|(tag, version)| {
                if let Some(version_str) = version.as_str() {
                    format!("{}: {}", tag.blue(), version_str)
                } else {
                    format!("{}: {}", tag.blue(), version)
                }
            }).collect::<Vec<_>>();
            print_grid(tags);
        }
    }
    
    // Print time information
    let publish_time = package_info.get("versions")
        .and_then(|versions| versions.get(target_version))
        .and_then(|version_info| version_info.get("publish_time"))
        .and_then(|v| v.as_u64());

    if let Some(publish_time) = publish_time {
        // Convert timestamp to datetime
        if let Some(published_time) = Utc.timestamp_opt(publish_time as i64 / 1000, 0).single() {
            let now = Utc::now();
            let duration = now.signed_duration_since(&published_time);
            
            let time_str = if duration.num_days() > 365 {
                format!("over a year ago")
            } else if duration.num_days() > 30 {
                format!("{} months ago", duration.num_days() / 30)
            } else if duration.num_days() > 0 {
                format!("{} days ago", duration.num_days())
            } else if duration.num_hours() > 0 {
                format!("{} hours ago", duration.num_hours())
            } else {
                format!("{} minutes ago", duration.num_minutes())
            };
            
            // Try to get publisher information from _npmUser field
            let npm_user = package_info.get("versions")
                .and_then(|versions| versions.get(target_version))
                .and_then(|version_info| version_info.get("_npmUser"))
                .and_then(|v| v.as_object());
            
            if let Some(npm_user) = npm_user {
                if let Some(publisher_name) = npm_user.get("name").and_then(|v| v.as_str()) {
                    if let Some(publisher_email) = npm_user.get("email").and_then(|v| v.as_str()) {
                        println!("\n{} {} by {} <{}>", "published", time_str.cyan(), publisher_name.blue(), publisher_email.white());
                    } else {
                        println!("\n{} {} by {}", "published", time_str.cyan(), publisher_name.blue());
                    }
                } else {
                    println!("\n{} {}", "published", time_str.cyan());
                }
            } else {
                println!("\n{} {}", "published", time_str.cyan());
            }
        }
    }
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_print_package_info() {
        let manifest = json!({
            "name": "test-package",
            "version": "1.0.0",
            "description": "A test package",
            "homepage": "https://example.com",
            "license": "MIT",
            "dependencies": {
                "lodash": "^4.17.21"
            },
            "scripts": {
                "test": "echo \"test\""
            }
        });

        // This test just ensures the function doesn't panic
        let result = print_package_info(&manifest, "test-package", &json!("1.0.0"));
        assert!(result.is_ok());
    }
}
