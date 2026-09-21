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
    local_backend_with_storage(runtime, Arc::new(StorageBackend::new()))
}

#[must_use]
pub fn local_backend_with_storage(
    runtime: ContainerEngine,
    storage: Arc<StorageBackend>,
) -> Arc<dyn TaskBackend> {
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
    docker_backend_with_storage(Arc::new(StorageBackend::new())).await
}

pub async fn docker_backend_with_storage(
    storage: Arc<StorageBackend>,
) -> miette::Result<Arc<dyn TaskBackend>> {
    let backend = DockerBackend::new(
        docker::Config::default(),
        storage,
        StoragePath::from_local(&env::temp_dir()),
    )
    .await
    .into_diagnostic()?;
    Ok(Arc::new(backend))
}

/// `TES Backend but env variabels
///
/// # Errors
/// `TES_URL`/`TES_STORAGE` are unset or malformed, or the TES server is not reachable.
pub async fn tes_backend() -> miette::Result<Arc<dyn TaskBackend>> {
    tes_backend_from_config(tes_config_from_env()?, Arc::new(StorageBackend::new())).await
}

/// Same as [`tes_backend`] but storage
///
/// # Errors
/// The TES server described by `config` is not reachable.
pub async fn tes_backend_from_config(
    config: TesBackendConfig,
    storage: Arc<StorageBackend>,
) -> miette::Result<Arc<dyn TaskBackend>> {
    let data_store = StoragePath::from_url(config.storage_url);
    let http = match config.token {
        Some(token) => tes::http::Config {
            auth: Some(tes::http::HttpAuthConfig::Bearer { token }),
            ..Default::default()
        },
        None => tes::http::Config::default(),
    };
    let tes_config = tes::Config::builder().url(config.url).http(http).build();
    let backend = TesBackend::new(tes_config, storage, data_store)
        .await
        .into_diagnostic()?;
    Ok(Arc::new(backend))
}

/// What a GA4GH TES server needs to submit work to it
#[derive(Debug, Clone)]
pub struct TesBackendConfig {
    pub url: Url,
    /// Where TES uploads/downloads through, e.g. `s3://my-bucket` -- cannot be local.
    pub storage_url: Url,
    pub token: Option<String>,
}

fn tes_config_from_env() -> miette::Result<TesBackendConfig> {
    let url = env::var("TES_URL")
        .map_err(|_| miette::miette!("TES_URL is not set (needed for --engine tes)"))?;
    let url = Url::parse(&url).into_diagnostic()?;
    let storage_url = tes_storage_url_from_env()?;
    let token = env::var("TES_TOKEN").ok();
    Ok(TesBackendConfig {
        url,
        storage_url,
        token,
    })
}

fn tes_storage_url_from_env() -> miette::Result<Url> {
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
        let err = tes_config_from_env().unwrap_err();
        assert!(err.to_string().contains("TES_URL"));
    }

    #[test]
    #[serial]
    fn tes_storage_url_reports_missing_tes_storage_clearly() {
        unsafe { env::remove_var("TES_STORAGE") };
        let err = tes_storage_url_from_env().unwrap_err();
        assert!(err.to_string().contains("TES_STORAGE"));
    }
}
