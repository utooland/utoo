use std::path::PathBuf;

use tokio::sync::mpsc;
use utoo_ruborist::progress::{BuildEvent, EventReceiver, PackageTarballInfo};

/// Owned version of PackageTarballInfo for channel transmission.
#[derive(Debug, Clone)]
pub struct OwnedPackageInfo {
    pub name: String,
    pub version: String,
    pub tarball_url: Option<String>,
}

impl From<&PackageTarballInfo<'_>> for OwnedPackageInfo {
    fn from(info: &PackageTarballInfo<'_>) -> Self {
        Self {
            name: info.name.to_string(),
            version: info.version.to_string(),
            tarball_url: info.tarball_url.map(|s| s.to_string()),
        }
    }
}

/// Message for clone worker.
#[derive(Debug)]
pub struct CloneMessage {
    pub info: OwnedPackageInfo,
    pub path: PathBuf,
    /// Parent package path (None for root-level dependencies)
    pub parent_path: Option<PathBuf>,
}

/// Pipeline receiver that wraps an inner receiver and forwards events to channels.
///
/// On `PackageResolved` → sends to download channel.
/// On `PackagePlaced` → sends to clone channel.
/// All events are also forwarded to the inner receiver (e.g. progress bar).
pub struct PipelineReceiver<R: EventReceiver> {
    download_tx: mpsc::UnboundedSender<OwnedPackageInfo>,
    clone_tx: mpsc::UnboundedSender<CloneMessage>,
    inner: R,
}

/// Channels returned when creating a PipelineReceiver.
pub struct PipelineChannels {
    pub download_rx: mpsc::UnboundedReceiver<OwnedPackageInfo>,
    pub clone_rx: mpsc::UnboundedReceiver<CloneMessage>,
}

impl<R: EventReceiver> PipelineReceiver<R> {
    /// Create a new pipeline receiver wrapping an inner receiver.
    pub fn new(inner: R) -> (Self, PipelineChannels) {
        let (download_tx, download_rx) = mpsc::unbounded_channel();
        let (clone_tx, clone_rx) = mpsc::unbounded_channel();
        (
            Self {
                download_tx,
                clone_tx,
                inner,
            },
            PipelineChannels {
                download_rx,
                clone_rx,
            },
        )
    }
}

impl<R: EventReceiver> EventReceiver for PipelineReceiver<R> {
    fn on_event(&self, event: BuildEvent<'_>) {
        // Forward to inner receiver first (for progress bar updates)
        self.inner.on_event(event.clone());

        match event {
            BuildEvent::PackageResolved(info)
                if info.tarball_url.is_some() && info.is_platform_compatible() =>
            {
                let _ = self.download_tx.send(OwnedPackageInfo::from(&info));
            }
            BuildEvent::PackagePlaced {
                package,
                path,
                parent_path,
            } if package.tarball_url.is_some() && package.is_platform_compatible() => {
                let _ = self.clone_tx.send(CloneMessage {
                    info: OwnedPackageInfo::from(&package),
                    path: path.to_path_buf(),
                    parent_path: parent_path.map(|p| p.to_path_buf()),
                });
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use utoo_ruborist::progress::NoopReceiver;

    #[test]
    fn test_pipeline_receiver_filters_events() {
        let (receiver, mut channels) = PipelineReceiver::new(NoopReceiver);

        // Should forward PackageResolved with tarball_url
        receiver.on_event(BuildEvent::PackageResolved(PackageTarballInfo {
            name: "react",
            version: "18.2.0",
            tarball_url: Some("https://registry.npmjs.org/react/-/react-18.2.0.tgz"),
            integrity: Some("sha512-xxx"),
            os: None,
            cpu: None,
        }));

        // Should not forward PackageResolved without tarball_url
        receiver.on_event(BuildEvent::PackageResolved(PackageTarballInfo {
            name: "local-pkg",
            version: "1.0.0",
            tarball_url: None,
            integrity: None,
            os: None,
            cpu: None,
        }));

        // Should not forward other events
        receiver.on_event(BuildEvent::PreloadStart { count: 10 });

        // Only one message should be in the download channel
        assert!(channels.download_rx.try_recv().is_ok());
        assert!(channels.download_rx.try_recv().is_err());
    }
}
