#![allow(clippy::all)]
use anyhow::{Context, Result};
use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use std::process::Command;
use std::{env, fs, path::PathBuf};

fn get_root_dir() -> PathBuf {
    env::current_dir().unwrap().join("../../")
}

// clean environment, package-lock.json ~/.cache/nm, ~/.npm node_modules
fn clean_environment() -> Result<()> {
    let root_dir = get_root_dir();
    let home_dir = env::var("HOME").unwrap();
    let utoo_cache_path = PathBuf::from(&home_dir).join(".cache/nm");
    let npm_cache_dir = PathBuf::from(&home_dir).join(".npm");

    let package_lock = root_dir.join("package-lock.json");
    let node_modules = root_dir.join("node_modules");

    if package_lock.exists() {
        fs::remove_file(package_lock).context("Failed to remove package-lock.json")?;
    }
    if node_modules.exists() {
        fs::remove_dir_all(node_modules).context("Failed to remove node_modules")?;
    }

    if utoo_cache_path.exists() {
        fs::remove_dir_all(utoo_cache_path).context("Failed to remove utoo cache")?;
    }

    // force clean npm cache
    Command::new("npm")
        .args(["cache", "clean", "--force"])
        .output()
        .context("Failed to clean npm cache")?;

    if npm_cache_dir.exists() {
        fs::remove_dir_all(&npm_cache_dir).context("Failed to remove .npm directory")?;
    }

    Ok(())
}

fn run_npm_deps_install() -> Result<()> {
    let root_dir = get_root_dir();
    let output = Command::new("npm")
        .args([
            "i",
            "--package-lock-only",
            "--registry=https://registry.npmmirror.com",
        ])
        .current_dir(&root_dir)
        .output()
        .context("Failed to run npm install")?;

    if !output.status.success() {
        eprintln!("npm error: {}", String::from_utf8_lossy(&output.stderr));
        return Err(anyhow::anyhow!("npm command failed"));
    }
    Ok(())
}

fn run_utoo_deps() -> Result<()> {
    let root_dir = get_root_dir();
    let utoo_path = root_dir.join("target/release/utoo");

    let output = Command::new(utoo_path)
        .arg("deps")
        .current_dir(&root_dir)
        .output()
        .context("Failed to run utoo deps")?;

    if !output.status.success() {
        eprintln!("utoo error: {}", String::from_utf8_lossy(&output.stderr));
        return Err(anyhow::anyhow!("utoo command failed"));
    }
    Ok(())
}

fn run_npm_full_install() -> Result<()> {
    let root_dir = get_root_dir();
    let output = Command::new("npm")
        .args(["i", "--registry=https://registry.npmmirror.com"])
        .current_dir(&root_dir)
        .output()
        .context("Failed to run npm install")?;

    if !output.status.success() {
        eprintln!("npm error: {}", String::from_utf8_lossy(&output.stderr));
        return Err(anyhow::anyhow!("npm install failed"));
    }
    Ok(())
}

fn run_utoo_install() -> Result<()> {
    let root_dir = get_root_dir();
    let utoo_path = root_dir.join("target/release/utoo");

    let output = Command::new(utoo_path)
        .arg("install")
        .current_dir(&root_dir)
        .output()
        .context("Failed to run utoo install")?;

    if !output.status.success() {
        eprintln!("utoo error: {}", String::from_utf8_lossy(&output.stderr));
        return Err(anyhow::anyhow!("utoo install failed"));
    }
    Ok(())
}

fn clean_node_modules() -> Result<()> {
    let root_dir = get_root_dir();
    let node_modules = root_dir.join("node_modules");

    if node_modules.exists() {
        fs::remove_dir_all(node_modules).context("Failed to remove node_modules")?;
    }
    Ok(())
}

fn benchmark_install(c: &mut Criterion) {
    let mut group = c.benchmark_group("Package Installation");
    group.sample_size(10);

    // warm_up_cache
    run_utoo_install().unwrap();

    group.bench_function(BenchmarkId::new("utoo", "install - with cache"), |b| {
        b.iter_with_setup(
            || clean_node_modules().unwrap(),
            |_| run_utoo_install().unwrap(),
        );
    });

    // warm_up_cache
    run_npm_full_install().unwrap();
    // still clean node_modules
    group.bench_function(BenchmarkId::new("npm", "install - with cache"), |b| {
        b.iter_with_setup(
            || clean_node_modules().unwrap(),
            |_| run_npm_full_install().unwrap(),
        );
    });

    // without cache
    group.bench_function(BenchmarkId::new("utoo", "install - without cache"), |b| {
        b.iter_with_setup(
            || clean_environment().unwrap(),
            |_| run_utoo_install().unwrap(),
        );
    });

    group.bench_function(BenchmarkId::new("npm", "install - without cache"), |b| {
        b.iter_with_setup(
            || clean_environment().unwrap(),
            |_| run_npm_full_install().unwrap(),
        );
    });

    group.finish();
}

fn clean_package_lock() -> Result<()> {
    let root_dir = get_root_dir();
    let package_lock = root_dir.join("package-lock.json");

    if package_lock.exists() {
        fs::remove_file(package_lock).context("Failed to remove package-lock.json")?;
    }
    Ok(())
}

fn benchmark_deps(c: &mut Criterion) {
    let mut group = c.benchmark_group("Dependencies Resolution");
    group.sample_size(10);

    run_npm_deps_install().unwrap();

    // with cache
    group.bench_function(
        BenchmarkId::new("npm", "package-lock-only - with cache"),
        |b| {
            b.iter_with_setup(
                || clean_package_lock().unwrap(),
                |_| run_npm_deps_install().unwrap(),
            );
        },
    );

    run_utoo_deps().unwrap();

    group.bench_function(BenchmarkId::new("utoo", "deps - with cache"), |b| {
        b.iter_with_setup(
            || clean_package_lock().unwrap(),
            |_| run_utoo_deps().unwrap(),
        );
    });

    // without cache
    group.bench_function(
        BenchmarkId::new("npm", "package-lock-only - without cache"),
        |b| {
            b.iter_with_setup(
                || clean_environment().unwrap(),
                |_| run_npm_deps_install().unwrap(),
            );
        },
    );

    group.bench_function(BenchmarkId::new("utoo", "deps - without cache"), |b| {
        b.iter_with_setup(
            || clean_environment().unwrap(),
            |_| run_utoo_deps().unwrap(),
        );
    });

    group.finish();
}

criterion_group!(benches, benchmark_install, benchmark_deps);
criterion_main!(benches);
