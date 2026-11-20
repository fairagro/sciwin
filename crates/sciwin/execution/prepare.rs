//! Turning what a user points at (a CWL file or a Workflow/Run RO-Crate) plus an optional job
//! file into a CWL file to submit and its inputs.

use crate::execution::{RunnerResult, rocrate};
use commonwl::engine::{InputObject, load_input_file_from_file};
use std::path::{Path, PathBuf};
use tracing::info;

/// A CWL file ready for [`super::WorkflowRunner::submit`] and its job inputs.
///
/// Keep it alive until the run has ended: for a zipped crate it owns the directory the
/// crate was extracted to, which `cwl_path` points into.
#[derive(Debug)]
pub struct PreparedRun {
    pub cwl_path: PathBuf,
    pub inputs: InputObject,
    /// Whether `file` was a Workflow/Run RO-Crate rather than a CWL file.
    pub from_crate: bool,
    _crate: Option<rocrate::ResolvedRun>,
}

/// Resolves `file`, a CWL file or a Workflow/Run RO-Crate (directory or `.zip` whose
/// `mainEntity` is CWL), and loads `input_file` on top of a crate's own default inputs.
/// Relative locations in the job file resolve against the CWL document's directory.
pub fn prepare_run(file: &Path, input_file: Option<&Path>) -> RunnerResult<PreparedRun> {
    let resolved = rocrate::looks_like_crate(file)
        .then(|| rocrate::resolve_target(file))
        .transpose()?;
    let (cwl_path, base_path) = match &resolved {
        // The CWL engine resolves a job file's relative `location:`s against the CWL
        // document's own directory, which for a crate can be a subdirectory of `base_dir`
        // (e.g. `workflows/demo/demo.cwl`), not `base_dir` itself.
        Some(resolved) => {
            let cwl_dir = resolved.cwl_path.parent().unwrap_or(&resolved.base_dir);
            (resolved.cwl_path.clone(), dunce::canonicalize(cwl_dir)?)
        }
        None => {
            let parent = file
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new("."));
            (file.to_path_buf(), dunce::canonicalize(parent)?)
        }
    };
    if resolved.is_some() {
        info!(
            "resolved RO-Crate {} to workflow {}",
            file.display(),
            cwl_path.display()
        );
    }

    let mut inputs = resolved
        .as_ref()
        .map(|resolved| resolved.default_inputs.clone())
        .unwrap_or_default();
    if let Some(input_file) = input_file {
        let overrides = load_input_file_from_file(input_file, base_path)?;
        inputs.inputs.extend(overrides.inputs);
        inputs.requirements = overrides.requirements;
        inputs.hints = overrides.hints;
    }

    Ok(PreparedRun {
        cwl_path,
        inputs,
        from_crate: resolved.is_some(),
        _crate: resolved,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_file_locations_resolve_against_the_cwl_file() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/hello_world");
        let cwl = root.join("workflows/main/main.cwl");

        let run = prepare_run(&cwl, Some(&root.join("inputs.yml"))).unwrap();

        assert_eq!(run.cwl_path, cwl);
        assert!(!run.from_crate);
        assert!(!run.inputs.inputs.is_empty());
    }

    #[test]
    fn a_zipped_crate_stays_extracted_while_the_run_is_alive() {
        let zip = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/test_workflow.zip");

        let run = prepare_run(&zip, None).unwrap();

        assert!(run.from_crate);
        assert!(run.cwl_path.ends_with("main/main.cwl"));
        assert!(run.cwl_path.is_file());
        let cwl_path = run.cwl_path.clone();
        drop(run);
        assert!(!cwl_path.exists());
    }

    #[test]
    fn without_job_file_a_cwl_file_has_no_inputs() {
        let cwl = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/hello_world/workflows/main/main.cwl");
        let run = prepare_run(&cwl, None).unwrap();
        assert!(run.inputs.inputs.is_empty());
    }
}
