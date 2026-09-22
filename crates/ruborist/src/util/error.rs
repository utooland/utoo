//! Cloneable failures for task reports and single-flight waiters.
use std::error::Error;
use std::sync::Arc;

/// Shares an owned error chain without formatting away its causes.
#[derive(Clone, Debug)]
pub struct SharedError(Arc<anyhow::Error>);

impl SharedError {
    /// Snapshot a provider's cause chain without requiring new Send/Sync bounds
    /// on its public error type. Native-only owned anyhow errors use `From`
    /// instead and retain their concrete sources as well.
    pub fn capture(error: &(dyn Error + 'static)) -> Self {
        let mut messages = vec![error.to_string()];
        let mut source = error.source();
        while let Some(error) = source {
            messages.push(error.to_string());
            source = error.source();
        }
        let mut messages = messages.into_iter().rev();
        let mut error = anyhow::Error::msg(messages.next().expect("an error has a message"));
        for message in messages {
            error = error.context(message);
        }
        Self::from(error)
    }

    /// Keep the historical one-line task message and attach the original cause.
    pub fn context(self, message: impl std::fmt::Display) -> Self {
        let message = format!("{message}: {self}");
        Self::from(anyhow::Error::new(self).context(message))
    }
}

impl From<anyhow::Error> for SharedError {
    fn from(error: anyhow::Error) -> Self {
        Self(Arc::new(error))
    }
}

impl std::fmt::Display for SharedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl Error for SharedError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.0.source()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_and_clone_retain_all_causes_without_sync_error_bounds() {
        #[derive(Debug)]
        struct ProviderError {
            cell: std::cell::Cell<u8>,
            source: std::io::Error,
        }
        impl std::fmt::Display for ProviderError {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "provider {}", self.cell.get())
            }
        }
        impl Error for ProviderError {
            fn source(&self) -> Option<&(dyn Error + 'static)> {
                Some(&self.source)
            }
        }
        let source = ProviderError {
            cell: std::cell::Cell::new(7),
            source: std::io::Error::other("connection reset"),
        };
        let error = SharedError::capture(&source).context("manifest task");
        let cloned = error.clone();
        drop(error);
        let error = anyhow::Error::new(cloned);
        assert_eq!(
            error.chain().map(ToString::to_string).collect::<Vec<_>>(),
            [
                "manifest task: provider 7",
                "provider 7",
                "connection reset"
            ]
        );
    }
}
