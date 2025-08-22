use crate::helper::package::parse_package_spec;
use crate::util::logger::log_verbose;
use crate::util::registry::{resolve, get_package_info};
use crate::util::format_print::print_grid;
use crate::model::package_manifest::PackageManifest;
use anyhow::{anyhow, Result};
use chrono::{TimeZone, Utc};
use owo_colors::OwoColorize;

/// View package information from registry, similar to npm view
pub async fn view(package_spec: &str) -> Result<()> {
    log_verbose(&format!("Viewing package: {package_spec}"));

    // Parse package specification
    let (name, version_spec) = parse_package_spec(package_spec);
    
    log_verbose(&format!("Resolved package: {name} (spec: {version_spec})"));

    // Get complete package information (like npm view)
    let package_info = get_package_info(name)
        .await
        .map_err(|e| anyhow!("Failed to fetch package info for {}, reason: {}", package_spec, e))?;

    // Get the specific version manifest if a version was specified
    let resolved_package = resolve(name, version_spec).await?;
    let version_manifest = resolved_package.manifest;

    // Create PackageManifest from the raw data
    let package_manifest = PackageManifest::from_package_info_and_manifest(
        &package_info,
        name,
        &version_manifest,
    );

    // Print package information in npm view format
    print_package_info(&package_manifest)?;

    Ok(())
}

/// Print package information in npm view style format
fn print_package_info(package_manifest: &PackageManifest) -> Result<()> {
    log_verbose(&format!("Target version: {}", package_manifest.version));
    
    print_package_header(package_manifest);
    print_package_description(package_manifest);
    print_keywords(package_manifest);
    print_dist_info(package_manifest);
    print_author_info(package_manifest);
    print_repository_info(package_manifest);
    print_bugs_info(package_manifest);
    print_dependencies(package_manifest);
    print_maintainers(package_manifest);
    print_dist_tags(package_manifest);
    print_publish_info(package_manifest);
    
    Ok(())
}

fn print_package_header(package_manifest: &PackageManifest) {
    let description = package_manifest.description.as_deref().unwrap_or("");
    let license = package_manifest.license.as_deref().unwrap_or("UNLICENSED");
    let deps_count = package_manifest.dependencies_count();
    let versions_count = package_manifest.versions_count;
    
    let deps_str = if deps_count == 0 { "none".to_string() } else { deps_count.to_string() };
    
    println!("\n{}@{} | {} | deps: {} | versions: {}", 
        package_manifest.name.bright_blue().bold(), 
        package_manifest.version.bright_green(), 
        license.yellow(), 
        deps_str.cyan(), 
        versions_count.magenta());
    
    if !description.is_empty() {
        println!("{}", description.white());
    }
}

fn print_package_description(package_manifest: &PackageManifest) {
    if let Some(homepage) = &package_manifest.homepage {
        println!("{}", homepage.blue().underline());
    }
    println!();
}

fn print_keywords(package_manifest: &PackageManifest) {
    if let Some(keywords) = package_manifest.keywords.as_ref().filter(|k| !k.is_empty()) {
        let keyword_str = keywords.join(", ");
        println!("{} {}", "keywords:".bright_cyan(), keyword_str.white());
    }
}

fn print_dist_info(package_manifest: &PackageManifest) {
    if let Some(dist) = package_manifest.dist.as_ref() {
        println!("\n{}", "dist".bright_yellow().bold());
        
        if let Some(tarball) = &dist.tarball {
            println!("{} {}", ".tarball:".cyan(), tarball.blue().underline());
        }
        
        if let Some(shasum) = &dist.shasum {
            println!("{} {}", ".shasum:".cyan(), shasum.green());
        }
        
        if let Some(integrity) = &dist.integrity {
            println!("{} {}", ".integrity:".cyan(), integrity.green());
        }
        
        if let Some(unpacked_size) = dist.unpacked_size {
            let size_mb = unpacked_size as f64 / 1024.0 / 1024.0;
            println!("{} {} MB", ".unpackedSize:".cyan(), format!("{size_mb:.1}").yellow());
        }
    }
}

fn print_author_info(package_manifest: &PackageManifest) {
    if let Some(author) = package_manifest.author.as_ref() {
        let author_line = match &author.email {
            Some(email) => format!("\n{} {} <{}>", "author:".bright_magenta(), author.name.white(), email.blue()),
            None => format!("\n{} {}", "author:".bright_magenta(), author.name.white()),
        };
        println!("{author_line}");
    }
}

fn print_repository_info(package_manifest: &PackageManifest) {
    if let Some(repo) = package_manifest.repository.as_ref() {
        println!("{} {}:{}", "repository:".bright_magenta(), repo.repo_type.green(), repo.url.blue().underline());
    }
}

fn print_bugs_info(package_manifest: &PackageManifest) {
    if let Some(bugs) = package_manifest.bugs.as_ref() {
        println!("{} {}", "bugs:".bright_magenta(), bugs.url.blue().underline());
    }
}

fn print_dependencies(package_manifest: &PackageManifest) {
    if let Some(dependencies) = package_manifest.dependencies.as_ref().filter(|d| !d.is_empty()) {
        println!("\n{} {}", "dependencies:".bright_yellow().bold(), dependencies.len().white());
        
        let show_count = 24;
        let show_deps: Vec<String> = dependencies.iter()
            .take(show_count)
            .map(|(dep_name, dep_version)| format!("{}: {}", dep_name.blue(), dep_version))
            .collect();
        
        print_grid(show_deps);
        
        if dependencies.len() > show_count {
            println!("(... and {} more.)", (dependencies.len() - show_count).to_string().white());
        }
    }
}

fn print_maintainers(package_manifest: &PackageManifest) {
    if let Some(maintainers) = package_manifest.maintainers.as_ref().filter(|m| !m.is_empty()) {
        println!("\n{}", "maintainers:".bright_yellow().bold());
        
        for maintainer in maintainers {
            let maintainer_line = match &maintainer.email {
                Some(email) => format!("- {} <{}>", maintainer.name.blue(), email.white()),
                None => format!("- {}", maintainer.name.blue()),
            };
            println!("{maintainer_line}");
        }
    }
}

fn print_dist_tags(package_manifest: &PackageManifest) {
    if let Some(dist_tags) = package_manifest.dist_tags.as_ref().filter(|d| !d.is_empty()) {
        println!("\n{}", "dist-tags:".bright_yellow().bold());
        
        let tags: Vec<String> = dist_tags.iter()
            .map(|(tag, version)| format!("{}: {}", tag.blue(), version))
            .collect();
        
        print_grid(tags);
    }
}

fn print_publish_info(package_manifest: &PackageManifest) {
    if let Some(publish_time) = package_manifest.get_publish_time(&package_manifest.version)
        && let Some(published_time) = Utc.timestamp_opt(publish_time as i64 / 1000, 0).single() {
            let time_str = format_time_ago(published_time);
            let publish_line = format_publish_line(&time_str, package_manifest);
            
            println!("\n{publish_line}");
        }
}

fn format_time_ago(published_time: chrono::DateTime<Utc>) -> String {
    let now = Utc::now();
    let duration = now.signed_duration_since(published_time);
    
    match duration.num_days() {
        days if days > 365 => "over a year ago".to_string(),
        days if days > 30 => format!("{} months ago", days / 30),
        days if days > 0 => format!("{days} days ago"),
        _ => match duration.num_hours() {
            hours if hours > 0 => format!("{hours} hours ago"),
            _ => format!("{} minutes ago", duration.num_minutes()),
        }
    }
}

fn format_publish_line(time_str: &str, package_manifest: &PackageManifest) -> String {
    match package_manifest.get_npm_user(&package_manifest.version) {
        Some(npm_user) => match &npm_user.email {
            Some(email) => format!("{} {} by {} <{}>", 
                "published", time_str.cyan(), npm_user.name.blue(), email.white()),
            None => format!("{} {} by {}", 
                "published", time_str.cyan(), npm_user.name.blue()),
        },
        None => format!("{} {}", "published", time_str.cyan()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::package_manifest::{PackageManifest, Author};
    use std::collections::HashMap;

    #[test]
    fn test_print_package_info() {
        // Create a test PackageManifest
        let package_manifest = PackageManifest {
            name: "test-package".to_string(),
            version: "1.0.0".to_string(),
            description: Some("A test package".to_string()),
            homepage: Some("https://example.com".to_string()),
            license: Some("MIT".to_string()),
            keywords: Some(vec!["test".to_string(), "package".to_string()]),
            dependencies: {
                let mut deps = HashMap::new();
                deps.insert("lodash".to_string(), "^4.17.21".to_string());
                Some(deps)
            },
            author: Some(Author {
                name: "Test Author".to_string(),
                email: Some("test@example.com".to_string()),
            }),
            repository: None,
            bugs: None,
            dist: None,
            maintainers: None,
            dist_tags: None,
            versions: None,
            versions_count: 1,
        };

        // This test just ensures the function doesn't panic
        let result = print_package_info(&package_manifest);
        assert!(result.is_ok());
    }
}
