# Pack Schema

一个用于 utoopack 配置文件的 JSON Schema 生成器，为 `utoopack.json` 配置文件提供类型提示和验证支持。

## ✨ 功能特性

- 🔧 **完整的配置支持**: 支持所有 pack-core 配置选项，包括复杂的 externals 配置
- 📝 **智能提示**: 为配置文件提供自动补全和验证
- 🎯 **类型安全**: 基于 Rust 类型系统生成准确的 JSON Schema
- 🔄 **架构同步**: 通过镜像类型与 pack-core 保持同步
- ⚡ **易于集成**: 支持多种 IDE 和编辑器的自动配置

## 🏗️ 架构设计

### 核心理念

Pack Schema 采用了**镜像类型架构**，既解决了用户提出的"避免重复配置维护"问题，又保持了 JsonSchema 兼容性：

1. **直接引用 pack-core**: 通过 `pub use pack_core::config::*` 重导出所有核心类型
2. **Schema 兼容类型**: 创建与 pack-core 结构完全对应但使用标准类型的 Schema 结构

### 类型映射策略

| pack-core 类型 | pack-schema 类型 | 说明 |
|---------------|-----------------|------|
| `RcStr` | `String` | JSON Schema 兼容的字符串类型 |
| `FxIndexMap<K,V>` | `HashMap<K,V>` | 标准 HashMap 替代 |
| `FxHashSet<T>` | `Vec<T>` | 数组表示集合 |
| turbo-tasks 类型 | 对应标准类型 | 移除运行时特定注解 |

## 🚀 使用方法

### 生成 Schema

```bash
# 使用 just (推荐)
just schema

# 或直接使用 cargo
cargo run -p pack-schema

```