use anyhow::{Context, Result};
use serde_json::Value;
use crate::util::registry::{resolve, get_package_info};
use crate::util::logger::log_verbose;
use crate::helper::package::parse_package_spec;
use chrono;
use owo_colors::OwoColorize;

/// View package information from registry, similar to npm view
pub async fn view(package_name: &str) -> Result<()> {
    log_verbose(&format!("Viewing package: {}", package_name));

    // Parse package specification
    let (name, version_spec) = parse_package_spec(package_name);
    
    log_verbose(&format!("Resolved package: {} (spec: {})", name, version_spec));

    // Get complete package information (like npm view)
    let package_info = get_package_info(name).await
        .context(format!("Failed to fetch package information for {}", package_name))?;

    // Get the specific version manifest if a version was specified
    let version_manifest = if version_spec != "*" {
        let resolved = resolve(name, version_spec).await?;
        Some(resolved.manifest)
    } else {
        None
    };

    // Print package information in npm view format
    print_package_info_npm_style(&package_info, name, version_manifest.as_ref())?;

    Ok(())
}

/// Print package information in npm view style format
fn print_package_info_npm_style(package_info: &Value, name: &str, version_manifest: Option<&Value>) -> Result<()> {
    // Get the latest version from package info
    let latest_version = package_info.get("dist-tags")
        .and_then(|tags| tags.get("latest"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    
    // Get the specific version if provided, otherwise use latest
    let target_version = if let Some(manifest) = version_manifest {
        manifest.get("version").and_then(|v| v.as_str()).unwrap_or(latest_version)
    } else {
        latest_version
    };
    
    // Get the target manifest
    let target_manifest = if let Some(manifest) = version_manifest {
        manifest
    } else {
        // Get the latest version manifest from package info
        package_info.get("versions")
            .and_then(|versions| versions.get(target_version))
            .unwrap_or(package_info)
    };
    
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
            println!("{} {:.1} MB", ".unpackedSize:".cyan(), size_mb.to_string().yellow());
        }
    }
    
    // Print author information
    let author_source = if version_manifest.is_some() {
        // For specific version, try version manifest first, then fallback to package info
        target_manifest.get("author")
            .or_else(|| package_info.get("author"))
    } else {
        // For latest version, use target manifest
        target_manifest.get("author")
    };
    
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
    let repo_source = if version_manifest.is_some() {
        target_manifest.get("repository")
            .or_else(|| package_info.get("repository"))
    } else {
        target_manifest.get("repository")
    };
    
    if let Some(repo) = repo_source.and_then(|v| v.as_object()) {
        if let Some(repo_type) = repo.get("type").and_then(|v| v.as_str()) {
            if let Some(repo_url) = repo.get("url").and_then(|v| v.as_str()) {
                println!("{} {}:{}", "repository:".bright_magenta(), repo_type.green(), repo_url.blue().underline());
            }
        }
    }
    
    // Print bugs information
    let bugs_source = if version_manifest.is_some() {
        target_manifest.get("bugs")
            .or_else(|| package_info.get("bugs"))
    } else {
        target_manifest.get("bugs")
    };
    
    if let Some(bugs) = bugs_source.and_then(|v| v.as_object()) {
        if let Some(bugs_url) = bugs.get("url").and_then(|v| v.as_str()) {
            println!("{} {}", "bugs:".bright_magenta(), bugs_url.blue().underline());
        }
    }
    
    // Print maintainers
    if let Some(maintainers) = package_info.get("maintainers").and_then(|v| v.as_array()) {
        if !maintainers.is_empty() {
            println!("\n{}", "maintainers:".bright_yellow().bold());
            for maintainer in maintainers {
                if let Some(name) = maintainer.get("name").and_then(|v| v.as_str()) {
                    if let Some(email) = maintainer.get("email").and_then(|v| v.as_str()) {
                        println!("- {} <{}>", name.white(), email.blue());
                    } else {
                        println!("- {}", name.white());
                    }
                }
            }
        }
    }
    
    // Print dist-tags
    if let Some(dist_tags) = package_info.get("dist-tags").and_then(|v| v.as_object()) {
        if !dist_tags.is_empty() {
            println!("\n{}", "dist-tags:".bright_yellow().bold());
            for (tag, version) in dist_tags {
                if let Some(version_str) = version.as_str() {
                    println!("{}: {}", tag.cyan(), version_str.bright_green());
                }
            }
        }
    }
    
    // Print time information
    let publish_time = if version_manifest.is_some() {
        // For specific version, try to get publish_time from the specific version in package_info
        package_info.get("versions")
            .and_then(|versions| versions.get(target_version))
            .and_then(|version_info| version_info.get("publish_time"))
            .and_then(|v| v.as_u64())
    } else {
        // For latest version, use target manifest
        target_manifest.get("publish_time").and_then(|v| v.as_u64())
    };
    
    if let Some(publish_time) = publish_time {
        // Convert timestamp to datetime
        if let Some(published_time) = chrono::DateTime::from_timestamp(publish_time as i64 / 1000, 0) {
            let now = chrono::Utc::now();
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
            let npm_user = if version_manifest.is_some() {
                // For specific version, try to get _npmUser from the specific version
                package_info.get("versions")
                    .and_then(|versions| versions.get(target_version))
                    .and_then(|version_info| version_info.get("_npmUser"))
                    .and_then(|v| v.as_object())
            } else {
                // For latest version, use target manifest
                target_manifest.get("_npmUser").and_then(|v| v.as_object())
            };
            
            if let Some(npm_user) = npm_user {
                if let Some(publisher_name) = npm_user.get("name").and_then(|v| v.as_str()) {
                    if let Some(publisher_email) = npm_user.get("email").and_then(|v| v.as_str()) {
                        println!("\n{} {} by {} <{}>", "published".bright_green(), time_str.white(), publisher_name.white(), publisher_email.blue());
                    } else {
                        println!("\n{} {} by {}", "published".bright_green(), time_str.white(), publisher_name.white());
                    }
                } else {
                    println!("\n{} {}", "published".bright_green(), time_str.white());
                }
            } else {
                println!("\n{} {}", "published".bright_green(), time_str.white());
            }
        }
    }
    
    Ok(())
}

/// Print package information in a readable format (legacy function)
fn print_package_info(manifest: &Value, name: &str, version: &str) -> Result<()> {
    println!("{}@{}", name, version);
    
    // Print basic information
    if let Some(description) = manifest.get("description").and_then(|v| v.as_str()) {
        println!("description: {}", description);
    }
    
    if let Some(homepage) = manifest.get("homepage").and_then(|v| v.as_str()) {
        println!("homepage: {}", homepage);
    }
    
    if let Some(repository) = manifest.get("repository") {
        if let Some(url) = repository.get("url").and_then(|v| v.as_str()) {
            println!("repository: {}", url);
        }
    }
    
    if let Some(author) = manifest.get("author") {
        if let Some(author_str) = author.as_str() {
            println!("author: {}", author_str);
        } else if let Some(author_obj) = author.as_object() {
            if let Some(name) = author_obj.get("name").and_then(|v| v.as_str()) {
                println!("author: {}", name);
            }
        }
    }
    
    if let Some(license) = manifest.get("license").and_then(|v| v.as_str()) {
        println!("license: {}", license);
    }
    
    // Print dependencies
    if let Some(dependencies) = manifest.get("dependencies").and_then(|v| v.as_object()) {
        if !dependencies.is_empty() {
            println!("dependencies:");
            for (dep_name, dep_version) in dependencies {
                println!("  {} {}", dep_name, dep_version);
            }
        }
    }
    
    if let Some(dev_dependencies) = manifest.get("devDependencies").and_then(|v| v.as_object()) {
        if !dev_dependencies.is_empty() {
            println!("devDependencies:");
            for (dep_name, dep_version) in dev_dependencies {
                println!("  {} {}", dep_name, dep_version);
            }
        }
    }
    
    if let Some(peer_dependencies) = manifest.get("peerDependencies").and_then(|v| v.as_object()) {
        if !peer_dependencies.is_empty() {
            println!("peerDependencies:");
            for (dep_name, dep_version) in peer_dependencies {
                println!("  {} {}", dep_name, dep_version);
            }
        }
    }
    
    if let Some(optional_dependencies) = manifest.get("optionalDependencies").and_then(|v| v.as_object()) {
        if !optional_dependencies.is_empty() {
            println!("optionalDependencies:");
            for (dep_name, dep_version) in optional_dependencies {
                println!("  {} {}", dep_name, dep_version);
            }
        }
    }
    
    // Print scripts
    if let Some(scripts) = manifest.get("scripts").and_then(|v| v.as_object()) {
        if !scripts.is_empty() {
            println!("scripts:");
            for (script_name, script_command) in scripts {
                println!("  {}: {}", script_name, script_command);
            }
        }
    }
    
    // Print engines
    if let Some(engines) = manifest.get("engines").and_then(|v| v.as_object()) {
        if !engines.is_empty() {
            println!("engines:");
            for (engine_name, engine_version) in engines {
                println!("  {} {}", engine_name, engine_version);
            }
        }
    }
    
    // Print keywords
    if let Some(keywords) = manifest.get("keywords").and_then(|v| v.as_array()) {
        if !keywords.is_empty() {
            let keyword_str = keywords
                .iter()
                .filter_map(|k| k.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            println!("keywords: {}", keyword_str);
        }
    }
    
    // Print dist information
    if let Some(dist) = manifest.get("dist") {
        if let Some(tarball) = dist.get("tarball").and_then(|v| v.as_str()) {
            println!("dist-tarball: {}", tarball);
        }
        if let Some(integrity) = dist.get("integrity").and_then(|v| v.as_str()) {
            println!("dist-integrity: {}", integrity);
        }
        if let Some(shasum) = dist.get("shasum").and_then(|v| v.as_str()) {
            println!("dist-shasum: {}", shasum);
        }
    }
    
    // Print time information
    if let Some(time) = manifest.get("time") {
        if let Some(published) = time.get("published").and_then(|v| v.as_str()) {
            println!("published: {}", published);
        }
        if let Some(created) = time.get("created").and_then(|v| v.as_str()) {
            println!("created: {}", created);
        }
        if let Some(modified) = time.get("modified").and_then(|v| v.as_str()) {
            println!("modified: {}", modified);
        }
    }
    
    // Print maintainers
    if let Some(maintainers) = manifest.get("maintainers").and_then(|v| v.as_array()) {
        if !maintainers.is_empty() {
            println!("maintainers:");
            for maintainer in maintainers {
                if let Some(name) = maintainer.get("name").and_then(|v| v.as_str()) {
                    println!("  {}", name);
                }
            }
        }
    }
    
    // Print contributors
    if let Some(contributors) = manifest.get("contributors").and_then(|v| v.as_array()) {
        if !contributors.is_empty() {
            println!("contributors:");
            for contributor in contributors {
                if let Some(name) = contributor.get("name").and_then(|v| v.as_str()) {
                    println!("  {}", name);
                }
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
        let result = print_package_info(&manifest, "test-package", "1.0.0");
        assert!(result.is_ok());
    }
}
