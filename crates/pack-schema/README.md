# Pack Schema

A JSON Schema generator for utoopack configuration files, providing type hints and validation support for `utoopack.json`.

## ✨ Features

- 🔧 **Complete configuration support**: Covers all pack-core configuration options, including complex externals configuration
- 📝 **Smart hints**: Provides auto-completion and validation for configuration files
- 🎯 **Type safety**: Generates accurate JSON Schema based on the Rust type system
- 🔄 **Schema sync**: Stays in sync with pack-core via mirrored types
- ⚡ **Easy integration**: Supports auto-configuration in various IDEs and editors

## 🏗️ Architecture

### Core Concept

Pack Schema adopts a **mirrored type architecture** that avoids duplicate configuration maintenance while preserving JsonSchema compatibility:

1. **Direct re-export from pack-core**: Re-exports all core types via `pub use pack_core::config::*`
2. **Schema-compatible types**: Creates Schema structs that mirror pack-core structures but use standard types

### Type Mapping Strategy

| pack-core type | pack-schema type | Description |
|---------------|-----------------|-------------|
| `RcStr` | `String` | JSON Schema compatible string type |
| `FxIndexMap<K,V>` | `HashMap<K,V>` | Standard HashMap replacement |
| `FxHashSet<T>` | `Vec<T>` | Array representation for sets |
| turbo-tasks types | Corresponding standard types | Runtime-specific annotations removed |

## 🚀 Usage

### Generating the Schema

```bash
# Using just (recommended)
just schema

# Or directly via cargo
cargo run -p pack-schema
```