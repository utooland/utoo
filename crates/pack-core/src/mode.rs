use serde::Deserialize;
use turbo_tasks::OperationValue;
use turbopack_ecmascript_runtime::RuntimeType;

use crate::shared::webpack_rules::WebpackLoaderBuiltinCondition;

/// The mode in which Next.js is running.
#[turbo_tasks::value(shared)]
#[turbo_tasks::task_input(contains_unresolved_vcs)]
#[derive(Default, Debug, Copy, Clone, Ord, PartialOrd, Hash, Deserialize, OperationValue)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    Development,
    #[default]
    Production,
}

impl Mode {
    /// Returns the NODE_ENV value for the current mode.
    pub fn node_env(&self) -> &'static str {
        match self {
            Mode::Development => "development",
            Mode::Production => "production",
        }
    }

    /// Returns conditions that can be used in the "rules" section for
    /// defining webpack loader configuration.
    pub fn webpack_loader_conditions(&self) -> impl Iterator<Item = WebpackLoaderBuiltinCondition> {
        match self {
            Mode::Development => [WebpackLoaderBuiltinCondition::Development],
            Mode::Production => [WebpackLoaderBuiltinCondition::Production],
        }
        .into_iter()
    }

    /// Returns the exports condition for the current mode.
    pub fn condition(&self) -> &'static str {
        match self {
            Mode::Development => "development",
            Mode::Production => "production",
        }
    }

    /// Returns true if the development React runtime should be used.
    pub fn is_react_development(&self) -> bool {
        match self {
            Mode::Development => true,
            Mode::Production => false,
        }
    }

    pub fn is_development(&self) -> bool {
        match self {
            Mode::Development => true,
            Mode::Production => false,
        }
    }

    pub fn is_production(&self) -> bool {
        match self {
            Mode::Development => false,
            Mode::Production => true,
        }
    }

    pub fn runtime_type(&self) -> RuntimeType {
        match self {
            Mode::Development => RuntimeType::Development,
            Mode::Production => RuntimeType::Production,
        }
    }
}
