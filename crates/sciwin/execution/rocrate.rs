use rocrate::{RoCrate, profile::Profile, views::View};
use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
};
use tempfile::TempDir;
use tracing::{debug, info};

use crate::execution::{RunnerError, RunnerResult};

const RUN_PROFILES: [fn(String) -> Profile; 3] =
    [Profile::ProcessRun, Profile::WorkflowRun, Profile::ProvenanceRun];

/// Where [`resolve_target`] found a runnable workflow, and what it takes to keep running it.
pub struct ResolvedRun {
    /// Path to the crate's main workflow entity, ready to hand to
    /// [`super::WorkflowRunner::submit`].
    pub cwl_path: PathBuf,
    /// Directory input files (job file entries, `InitialWorkDir` sources, ...) resolve against.
    pub base_dir: PathBuf,
    /// Holds a zip's extraction directory alive for as long as the resolved run is -- execution
    /// reads crate files off disk long after this function returns, not just during it.
    _tempdir: Option<TempDir>,
}

/// Accepts a Workflow RO-Crate, or a Run Crate built on top of one, as a directory or a `.zip`
/// archive, and resolves it to a plain CWL path plus the directory it lives in
pub fn resolve_target(dir_or_zip: &Path) -> RunnerResult<ResolvedRun> {
    let (base_dir, ro_crate, tempdir) = if dir_or_zip.is_dir() {
        debug!("reading RO-Crate from directory {}", dir_or_zip.display());
        let ro_crate = RoCrate::from_directory(dir_or_zip)?;
        (dir_or_zip.to_path_buf(), ro_crate, None)
    } else if dir_or_zip.extension() == Some(OsStr::new("zip")) {
        let tempdir = tempfile::tempdir()?;
        debug!(
            "extracting RO-Crate archive {} to {}",
            dir_or_zip.display(),
            tempdir.path().display()
        );
        let ro_crate = rocrate::io::unzip(dir_or_zip, tempdir.path())?;
        (tempdir.path().to_path_buf(), ro_crate, Some(tempdir))
    } else {
        return Err(RunnerError::Unknown(anyhow::anyhow!(
            "{} is neither a directory nor a .zip archive",
            dir_or_zip.display()
        )));
    };

    let is_workflow_ro_crate = ro_crate.claims(&Profile::WorkflowRoCrate(String::new()));
    let is_run_crate = RUN_PROFILES
        .iter()
        .any(|profile| ro_crate.claims(&profile(String::new())));
    if !is_workflow_ro_crate && !is_run_crate {
        return Err(RunnerError::Unknown(anyhow::anyhow!(
            "{} does not claim the Workflow RO-Crate profile (or a Run Crate profile built on \
             it)",
            dir_or_zip.display()
        )));
    }

    let Some(wroc) = ro_crate.workflow() else {
        return Err(RunnerError::Unknown(anyhow::anyhow!(
            "RO-Crate has no mainEntity workflow"
        )));
    };
    debug!("RO-Crate successfully identified as Workflow RO-Crate");

    if let Some(lang) = wroc.language() {
        info!("workflow language: {:?}", lang.name());
    }

    let cwl_path = base_dir.join(wroc.id());

    let missing = ro_crate.missing_parts(&base_dir);
    if !missing.is_empty() {
        return Err(RunnerError::Unknown(anyhow::anyhow!(
            "RO-Crate is missing file(s) referenced by its metadata: {}",
            missing.join(", ")
        )));
    }

    Ok(ResolvedRun {
        cwl_path,
        base_dir,
        _tempdir: tempdir,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_target_smoke_zip() {
        let zip_path = PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../testdata/test_workflow.zip"
        ));

        let resolved = resolve_target(&zip_path).expect("should resolve a Workflow RO-Crate zip");

        assert!(resolved.cwl_path.ends_with("workflows/demo/demo.cwl"));
        assert!(resolved.cwl_path.is_file());
        assert!(resolved._tempdir.is_some());
        // sibling crate files, unpacked alongside the resolved workflow
        assert!(resolved.base_dir.join("code/plot_map.py").is_file());
        assert!(
            resolved
                .base_dir
                .join("data/braunschweig/stadtbezirke.shp")
                .is_file()
        );
    }
}
