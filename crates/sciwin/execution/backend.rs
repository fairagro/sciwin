//! Builds the [`Arc<dyn TaskBackend>`] a [`crate::execution::TaskRunner`] runs against, for
//! whichever engine a frontend selected. Kept here (not in `crates/cli`) so the GUI can select
//! an engine the same way later -- business logic belongs in this crate, both frontends share
//! it.

use commonwl::engine::{ContainerEngine, DockerBackend, LocalBackend, TaskBackend, TesBackend};
use commonwl::storage::{StorageBackend, StoragePath};
use crankshaft::config::backend::{docker, tes};
use miette::IntoDiagnostic;
use std::{env, sync::Arc};
use url::Url;

/// `Engine::Local`: only containerizes steps with a `DockerRequirement`, via `runtime`.
#[must_use]
pub fn local_backend(runtime: ContainerEngine) -> Arc<dyn TaskBackend> {
    let storage = Arc::new(StorageBackend::new());
    Arc::new(LocalBackend::new(
        runtime,
        storage,
        StoragePath::from_local(&env::temp_dir()),
    ))
}

/// `Engine::Docker`: every step containerized via Docker directly (bollard), regardless of any
/// `--runtime` selection.
///
/// # Errors
/// The Docker daemon is not reachable.
pub async fn docker_backend() -> miette::Result<Arc<dyn TaskBackend>> {
    let storage = Arc::new(StorageBackend::new());
    let backend = DockerBackend::new(
        docker::Config::default(),
        storage,
        StoragePath::from_local(&env::temp_dir()),
    )
    .await
    .into_diagnostic()?;
    Ok(Arc::new(backend))
}

/// `Engine::Tes`: submits to a GA4GH TES server. Reads `TES_URL` (required), `TES_STORAGE`
/// (required, e.g. `s3://my-bucket` -- TES uploads/downloads through this, it cannot be local),
/// and `TES_TOKEN` (optional bearer token) from the environment.
///
/// # Errors
/// `TES_URL`/`TES_STORAGE` are unset or malformed, or the TES server is not reachable.
pub async fn tes_backend() -> miette::Result<Arc<dyn TaskBackend>> {
    let storage = Arc::new(StorageBackend::new());
    let data_store = StoragePath::from_url(tes_storage_url()?);
    let backend = TesBackend::new(tes_config()?, storage, data_store)
        .await
        .into_diagnostic()?;
    Ok(Arc::new(backend))
}

fn tes_config() -> miette::Result<tes::Config> {
    let url = env::var("TES_URL")
        .map_err(|_| miette::miette!("TES_URL is not set (needed for --engine tes)"))?;
    let url = Url::parse(&url).into_diagnostic()?;

    let http = match env::var("TES_TOKEN") {
        Ok(token) => tes::http::Config {
            auth: Some(tes::http::HttpAuthConfig::Bearer { token }),
            ..Default::default()
        },
        Err(_) => tes::http::Config::default(),
    };

    Ok(tes::Config::builder().url(url).http(http).build())
}

fn tes_storage_url() -> miette::Result<Url> {
    let raw = env::var("TES_STORAGE").map_err(|_| {
        miette::miette!("TES_STORAGE is not set (needed for --engine tes, e.g. s3://my-bucket)")
    })?;
    Url::parse(&raw).into_diagnostic()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[test]
    #[serial]
    fn tes_config_reports_missing_tes_url_clearly() {
        // SAFETY: `#[serial]` prevents this from racing other env-var-touching tests in the
        // same binary; `TES_URL` is not read anywhere outside this module.
        unsafe { env::remove_var("TES_URL") };
        let err = tes_config().unwrap_err();
        assert!(err.to_string().contains("TES_URL"));
    }

    #[test]
    #[serial]
    fn tes_storage_url_reports_missing_tes_storage_clearly() {
        unsafe { env::remove_var("TES_STORAGE") };
        let err = tes_storage_url().unwrap_err();
        assert!(err.to_string().contains("TES_STORAGE"));
    }
}
