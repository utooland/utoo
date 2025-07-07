# Utoo Project Tasks
# Use `just --list` to see all available tasks

# Generate JSON Schema for project configuration
schema:
    cargo run -p pack-schema

# Build all crates
build:
    cargo build

# Build all crates in release mode
build-release:
    cargo build --release

# Run tests for all crates
test:
    cargo test

# Run tests for a specific crate
test-crate crate:
    cargo test -p {{crate}}

# Run clippy linter
lint:
    cargo clippy -- -D warnings

# Format code
fmt:
    cargo fmt

# Check formatting without making changes
fmt-check:
    cargo fmt --check

# Clean build artifacts
clean:
    cargo clean

update-submodule:
    git submodule update --init
