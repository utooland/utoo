use std::path::PathBuf;

use utoo_ruborist::progress::{BuildEvent, EventReceiver, PackageTarballInfo};

use crate::service::install_scheduler::InstallScheduler;

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

/// Pipeline receiver that wraps an inner receiver and forwards events to channels.
///
/// On `PackageResolved` → asks the install scheduler to prefetch the tarball.
/// On `PackagePlaced` → asks the install scheduler to pre-clone into node_modules.
/// All events are also forwarded to the inner receiver (e.g. progress bar).
pub struct PipelineReceiver<R: EventReceiver> {
    scheduler: InstallScheduler,
    cwd: PathBuf,
    inner: R,
}

impl<R: EventReceiver> PipelineReceiver<R> {
    /// Create a new pipeline receiver wrapping an inner receiver.
    pub fn new(inner: R, scheduler: InstallScheduler, cwd: PathBuf) -> Self {
        Self {
            scheduler,
            cwd,
            inner,
        }
    }
}

impl<R: EventReceiver> EventReceiver for PipelineReceiver<R> {
    fn on_event(&self, event: BuildEvent<'_>) {
        // Forward to inner receiver first (for progress bar updates)
        self.inner.on_event(event);

        match event {
            BuildEvent::PackageResolved(info)
                if info.tarball_url.is_some() && info.is_platform_compatible() =>
            {
                let info = OwnedPackageInfo::from(&info);
                let Some(tarball_url) = info.tarball_url else {
                    return;
                };
                self.scheduler
                    .prefetch_download(info.name, info.version, tarball_url);
            }
            BuildEvent::PackagePlaced {
                package,
                path,
                parent_path,
            } if package.tarball_url.is_some() && package.is_platform_compatible() => {
                let info = OwnedPackageInfo::from(&package);
                let Some(tarball_url) = info.tarball_url else {
                    return;
                };
                self.scheduler.prefetch_clone(
                    info.name,
                    info.version,
                    tarball_url,
                    self.cwd.join(path),
                    parent_path.map(|p| self.cwd.join(p)),
                );
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
        let receiver = PipelineReceiver::new(
            NoopReceiver,
            crate::service::install_scheduler::InstallScheduler::closed_for_test(),
            std::env::current_dir().unwrap(),
        );

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
        receiver.on_event(BuildEvent::LevelStart { node_count: 10 });
    }
}
