use anyhow::{Result, anyhow};
use chrono::Utc;
use owo_colors::OwoColorize;
use utoo_ruborist::manifest::{FullManifest, VersionManifest};
use utoo_ruborist::service::{MetadataFormat, fetch_full_manifest_fresh};
use utoo_ruborist::util::parse_package_spec;

use crate::service::auth;
use crate::util::format_print::print_grid;
use crate::util::user_config::get_registry;

/// View package information from registry, similar to npm view
pub async fn view(package_spec: &str) -> Result<()> {
    let registry_url = get_registry();
    view_with_registry(package_spec, &registry_url).await
}

async fn view_with_registry(package_spec: &str, registry_url: &str) -> Result<()> {
    tracing::debug!("Viewing package: {package_spec}");

    // Parse package specification
    let (name, version_spec) = parse_package_spec(package_spec);

    tracing::debug!("Resolved package: {name} (spec: {version_spec})");

    // Fetch full manifest directly from registry (Complete format for display, no ETag).
    // token_for_url applies the leak guard: a token only when registry_url is
    // the configured registry host.
    let token = auth::token_for_url(registry_url).await;
    let (full_manifest, _etag) = fetch_full_manifest_fresh(
        registry_url,
        name,
        MetadataFormat::Complete,
        token.as_deref(),
    )
    .await
    .map_err(|e| anyhow!("Failed to fetch package info for {}: {}", package_spec, e))?;

    tracing::debug!("Fetched package info: {full_manifest:?}");

    // Resolve version and get full VersionManifest (with all display fields)
    let resolved_version = utoo_ruborist::resolver::version::resolve_target_version(
        (&full_manifest).into(),
        version_spec,
    )
    .map_err(|e| {
        anyhow!(
            "Version resolution failed for {}@{}: {}",
            name,
            version_spec,
            e
        )
    })?;

    let version_manifest: VersionManifest = full_manifest
        .get_full_version(&resolved_version)
        .ok_or_else(|| anyhow!("Version {} not found for {}", resolved_version, name))?;

    // Print package information in npm view format
    print_package_info(&full_manifest, &version_manifest)?;

    Ok(())
}

/// Print package information in npm view style format using strong types
fn print_package_info(
    full_manifest: &FullManifest,
    version_manifest: &VersionManifest,
) -> Result<()> {
    tracing::debug!("Target version: {}", version_manifest.version);

    print_package_header(full_manifest, version_manifest);
    print_package_description(full_manifest, version_manifest);
    print_keywords(full_manifest, version_manifest);
    print_dist_info(version_manifest);
    print_author_info(full_manifest, version_manifest);
    print_repository_info(full_manifest, version_manifest);
    print_bugs_info(full_manifest, version_manifest);
    print_dependencies(version_manifest);
    print_maintainers(full_manifest);
    print_dist_tags(full_manifest);
    print_publish_info(full_manifest, version_manifest);

    Ok(())
}

fn print_package_header(full_manifest: &FullManifest, version_manifest: &VersionManifest) {
    // Use description from version manifest or fallback to full manifest
    let description = version_manifest
        .description
        .as_deref()
        .or(full_manifest.description.as_deref())
        .unwrap_or("");

    // Use license from version manifest or fallback to full manifest
    let license = version_manifest
        .license
        .as_deref()
        .or(full_manifest.license.as_deref())
        .unwrap_or("UNLICENSED");

    let deps_count = version_manifest
        .dependencies
        .as_ref()
        .map(|d| d.len())
        .unwrap_or(0);
    let versions_count = full_manifest.versions.len();

    let deps_str = if deps_count == 0 {
        "none".to_string()
    } else {
        deps_count.to_string()
    };

    println!(
        "\n{}@{} | {} | deps: {} | versions: {}",
        version_manifest.name.bright_blue().bold(),
        version_manifest.version.bright_green(),
        license.yellow(),
        deps_str.cyan(),
        versions_count.magenta()
    );

    if !description.is_empty() {
        println!("{}", description.white());
    }
}

fn print_package_description(full_manifest: &FullManifest, version_manifest: &VersionManifest) {
    // Use homepage from version manifest or fallback to full manifest
    let homepage = version_manifest
        .homepage
        .as_ref()
        .or(full_manifest.homepage.as_ref());

    if let Some(homepage) = homepage {
        println!("{}", homepage.blue().underline());
    }
    println!();
}

fn print_keywords(full_manifest: &FullManifest, version_manifest: &VersionManifest) {
    // Use keywords from version manifest or fallback to full manifest
    let keywords = version_manifest
        .keywords
        .as_ref()
        .or(full_manifest.keywords.as_ref())
        .filter(|k| !k.is_empty());

    if let Some(keywords) = keywords {
        let keyword_str = keywords.join(", ");
        println!("{} {}", "keywords:".bright_cyan(), keyword_str.white());
    }
}

fn print_dist_info(version_manifest: &VersionManifest) {
    if let Some(tarball) = &version_manifest.dist.tarball {
        println!("\n{}", "dist".bright_yellow().bold());
        println!("{} {}", ".tarball:".cyan(), tarball.blue().underline());
    }

    if let Some(shasum) = &version_manifest.dist.shasum {
        println!("{} {}", ".shasum:".cyan(), shasum.green());
    }

    if let Some(integrity) = &version_manifest.dist.integrity {
        println!("{} {}", ".integrity:".cyan(), integrity.green());
    }

    if let Some(unpacked_size) = version_manifest.dist.unpacked_size {
        let size_mb = unpacked_size as f64 / 1024.0 / 1024.0;
        println!(
            "{} {} MB",
            ".unpackedSize:".cyan(),
            format!("{size_mb:.1}").yellow()
        );
    }
}

fn print_author_info(full_manifest: &FullManifest, version_manifest: &VersionManifest) {
    // Use author from version manifest or fallback to full manifest
    let author = version_manifest
        .author
        .as_ref()
        .or(full_manifest.author.as_ref());

    if let Some(author) = author {
        let author_line = match &author.email {
            Some(email) => format!(
                "\n{} {} <{}>",
                "author:".bright_magenta(),
                author.name.white(),
                email.blue()
            ),
            None => format!("\n{} {}", "author:".bright_magenta(), author.name.white()),
        };
        println!("{author_line}");
    }
}

fn print_repository_info(full_manifest: &FullManifest, version_manifest: &VersionManifest) {
    // Use repository from version manifest or fallback to full manifest
    let repository = version_manifest
        .repository
        .as_ref()
        .or(full_manifest.repository.as_ref());

    if let Some(repo) = repository {
        println!(
            "{} {}:{}",
            "repository:".bright_magenta(),
            repo.repo_type.green(),
            repo.url.blue().underline()
        );
    }
}

fn print_bugs_info(full_manifest: &FullManifest, version_manifest: &VersionManifest) {
    // Use bugs from version manifest or fallback to full manifest
    let bugs = version_manifest
        .bugs
        .as_ref()
        .or(full_manifest.bugs.as_ref());

    if let Some(bugs) = bugs {
        println!(
            "{} {}",
            "bugs:".bright_magenta(),
            bugs.url.blue().underline()
        );
    }
}

fn print_dependencies(version_manifest: &VersionManifest) {
    if let Some(dependencies) = version_manifest
        .dependencies
        .as_ref()
        .filter(|d| !d.is_empty())
    {
        println!(
            "\n{} {}",
            "dependencies:".bright_yellow().bold(),
            dependencies.len().white()
        );

        let show_count = 24;
        let show_deps: Vec<String> = dependencies
            .iter()
            .take(show_count)
            .map(|(dep_name, dep_version)| format!("{}: {}", dep_name.blue(), dep_version))
            .collect();

        print_grid(show_deps);

        if dependencies.len() > show_count {
            println!(
                "(... and {} more.)",
                (dependencies.len() - show_count).to_string().white()
            );
        }
    }
}

fn print_maintainers(full_manifest: &FullManifest) {
    if let Some(maintainers) = full_manifest.maintainers.as_ref().filter(|m| !m.is_empty()) {
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

fn print_dist_tags(full_manifest: &FullManifest) {
    if !full_manifest.dist_tags.is_empty() {
        println!("\n{}", "dist-tags:".bright_yellow().bold());

        let tags: Vec<String> = full_manifest
            .dist_tags
            .iter()
            .map(|(tag, version)| format!("{}: {}", tag.blue(), version))
            .collect();

        print_grid(tags);
    }
}

fn print_publish_info(full_manifest: &FullManifest, version_manifest: &VersionManifest) {
    // Get publish time from time info
    if let Some(time_str) = full_manifest.time.get(&version_manifest.version)
        && let Ok(published_time) = chrono::DateTime::parse_from_rfc3339(time_str)
    {
        let time_str = format_time_ago(published_time.with_timezone(&Utc));
        let publish_line = format_publish_line(&time_str, version_manifest);
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
        },
    }
}

fn format_publish_line(time_str: &str, version_manifest: &VersionManifest) -> String {
    match &version_manifest.npm_user {
        Some(npm_user) => match &npm_user.email {
            Some(email) => format!(
                "{} {} by {} <{}>",
                "published",
                time_str.cyan(),
                npm_user.name.blue(),
                email.white()
            ),
            None => format!(
                "{} {} by {}",
                "published",
                time_str.cyan(),
                npm_user.name.blue()
            ),
        },
        None => format!("{} {}", "published", time_str.cyan()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use utoo_ruborist::manifest::Dist;

    #[test]
    fn test_print_package_info() {
        // Create test data
        let full_manifest = FullManifest {
            name: "test-package".to_string(),
            maintainers: Some(vec![]),
            ..Default::default()
        };

        let version_manifest = VersionManifest {
            name: "test-package".to_string(),
            version: "1.0.0".to_string(),
            description: Some("A test package".to_string()),
            dist: Dist::default(),
            ..Default::default()
        };

        // This test just ensures the function doesn't panic
        let result = print_package_info(&full_manifest, &version_manifest);
        assert!(result.is_ok());
    }

    /// E2E test: verify that calling view twice works correctly.
    /// Previously, the second call would fail with "304 Not Modified" error
    /// because the registry service used ETag caching.
    #[tokio::test]
    async fn test_view_twice_no_304_error() {
        use mockito::Matcher;

        let mut server = mockito::Server::new_async().await;
        let manifest = r#"{
            "name": "is-odd",
            "description": "mock package",
            "dist-tags": { "latest": "1.0.0" },
            "versions": {
                "1.0.0": {
                    "name": "is-odd",
                    "version": "1.0.0",
                    "description": "mock package",
                    "dist": {}
                }
            }
        }"#;
        let mock = server
            .mock("GET", "/is-odd")
            .match_header("accept", "application/json")
            .match_header("if-none-match", Matcher::Missing)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_header("etag", "\"mock-etag\"")
            .with_body(manifest)
            .expect(2)
            .create_async()
            .await;

        // First view - should succeed
        let result1 = view_with_registry("is-odd", &server.url()).await;
        assert!(result1.is_ok(), "First view failed: {:?}", result1.err());

        // Second view - should also succeed (not fail with 304 error)
        let result2 = view_with_registry("is-odd", &server.url()).await;
        assert!(result2.is_ok(), "Second view failed: {:?}", result2.err());
        mock.assert_async().await;
    }
}
