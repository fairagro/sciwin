use commonwl::{
    engine::InputObject,
    files::{Directory, File, FileOrDirectory},
    inputs::DefaultValue,
};
use rocrate::{
    RoCrate,
    profile::Profile,
    views::{View, Workflow},
};
use std::{
    collections::HashMap,
    ffi::OsStr,
    path::{Path, PathBuf},
};
use tempfile::TempDir;
use tracing::{debug, info};

use crate::execution::{RunnerError, RunnerResult};

const RUN_PROFILES: [fn(String) -> Profile; 3] = [
    Profile::ProcessRun,
    Profile::WorkflowRun,
    Profile::ProvenanceRun,
];

/// Where [`resolve_target`] found a runnable workflow, and what it takes to keep running it.
#[derive(Debug)]
pub struct ResolvedRun {
    /// Path to the crate's main workflow entity, ready to hand to
    /// [`super::WorkflowRunner::submit`].
    pub cwl_path: PathBuf,
    /// Directory input files (job file entries, `InitialWorkDir` sources, ...) resolve against.
    pub base_dir: PathBuf,
    /// Input values the crate's own `FormalParameter`s supply
    pub default_inputs: InputObject,
    /// Holds a zip's extraction directory alive for as long as the resolved run is -- execution
    /// reads crate files off disk long after this function returns, not just during it.
    _tempdir: Option<TempDir>,
}

/// Whether `path` should go through [`resolve_target`] rather than be executed directly as a
/// plain CWL file -- a directory, or something with a `.zip` extension.
#[must_use]
pub fn looks_like_crate(path: &Path) -> bool {
    path.is_dir() || path.extension() == Some(OsStr::new("zip"))
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

    match wroc.language() {
        Some(lang)
            if lang
                .alternate_name()
                .is_some_and(|name| name.eq_ignore_ascii_case("cwl")) =>
        {
            info!("workflow language confirmed as CWL");
        }
        Some(lang) => {
            return Err(RunnerError::Unknown(anyhow::anyhow!(
                "workflow's programmingLanguage is `{}`, not CWL. only CWL execution is \
                 supported",
                lang.name()
                    .or_else(|| lang.alternate_name())
                    .unwrap_or("unknown")
            )));
        }
        None => {
            return Err(RunnerError::Unknown(anyhow::anyhow!(
                "RO-Crate's workflow declares no programmingLanguage, cannot confirm it is CWL"
            )));
        }
    }

    let cwl_path = base_dir.join(wroc.id());

    let missing = ro_crate.missing_parts(&base_dir);
    if !missing.is_empty() {
        return Err(RunnerError::Unknown(anyhow::anyhow!(
            "RO-Crate is missing file(s) referenced by its metadata: {}",
            missing.join(", ")
        )));
    }

    let default_inputs = crate_default_inputs(&wroc, &base_dir);

    Ok(ResolvedRun {
        cwl_path,
        base_dir,
        default_inputs,
        _tempdir: tempdir,
    })
}

/// Input values derived from the workflow's own `FormalParameter`s work_examples
fn crate_default_inputs(workflow: &Workflow<'_>, base_dir: &Path) -> InputObject {
    let mut inputs = HashMap::new();
    for param in workflow.inputs() {
        let (Some(name), Some(example)) = (param.name(), param.work_example()) else {
            continue;
        };
        let path = base_dir.join(&example.id);
        let location = path.to_string_lossy().into_owned();
        let value = if example.has_type("File") && path.is_file() {
            DefaultValue::FileOrDirectory(FileOrDirectory::File(
                File::builder().location(location).build(),
            ))
        } else if example.has_type("Dataset") && path.is_dir() {
            DefaultValue::FileOrDirectory(FileOrDirectory::Directory(
                Directory::builder().location(location).build(),
            ))
        } else {
            continue;
        };
        inputs.insert(name.to_string(), value);
    }
    InputObject {
        inputs,
        ..Default::default()
    }
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

        assert!(resolved.cwl_path.ends_with("main/main.cwl"));
        assert!(resolved.cwl_path.is_file());
        assert!(resolved._tempdir.is_some());
        // sibling crate parts, unpacked alongside the resolved workflow
        assert!(resolved.base_dir.join("calculation/calculation.cwl").is_file());
        assert!(resolved.base_dir.join("calculation/calculation.py").is_file());
        assert!(resolved.base_dir.join("plot/plot.cwl").is_file());
        assert!(resolved.base_dir.join("plot/plot.py").is_file());
        assert!(resolved.base_dir.join("population.csv").is_file());
    }

    #[test]
    fn test_resolve_target_rejects_non_cwl_language() {
        use rocrate::build::Entity;

        let crate_ = RoCrate::builder()
            .date_published("2026-01-01")
            .name("Non-CWL workflow")
            .conforms_to(Profile::WorkflowRoCrate("1.0".into()))
            .main_workflow(
                Entity::new(
                    "main.nf",
                    &["File", "SoftwareSourceCode", "ComputationalWorkflow"],
                )
                .set("name", "Example workflow")
                .reference("programmingLanguage", "#nextflow"),
            )
            .entity(
                Entity::new("#nextflow", "ComputerLanguage")
                    .set("name", "Nextflow")
                    .set("alternateName", "NFL"),
            )
            .build();

        let dir = tempfile::tempdir().unwrap();
        crate_.write_directory(dir.path()).unwrap();
        std::fs::write(dir.path().join("main.nf"), "// not cwl").unwrap();

        let err = resolve_target(dir.path()).unwrap_err();
        assert!(err.to_string().contains("CWL"), "{err}");
    }

    #[test]
    fn test_resolve_target_default_inputs_from_work_examples() {
        use rocrate::build::Entity;

        let crate_ = RoCrate::builder()
            .date_published("2026-01-01")
            .name("Run Crate with recorded example inputs")
            .conforms_to(Profile::WorkflowRoCrate("1.0".into()))
            .conforms_to(Profile::WorkflowRun("0.5".into()))
            .main_workflow(
                Entity::new(
                    "main.cwl",
                    &["File", "SoftwareSourceCode", "ComputationalWorkflow"],
                )
                .set("name", "Example workflow")
                .reference("programmingLanguage", "#cwl")
                .references("input", ["#data", "#phantom", "#scalar"]),
            )
            .entity(
                Entity::new("#cwl", "ComputerLanguage")
                    .set("name", "Common Workflow Language")
                    .set("alternateName", "CWL"),
            )
            // A File `workExample` the crate actually ships -- becomes a File default.
            .entity(
                Entity::new("#data", "FormalParameter")
                    .set("name", "data")
                    .reference("workExample", "data.csv"),
            )
            .part(Entity::new("data.csv", "File"))
            // A File `workExample` that's declared but not actually on disk -- skipped, not
            // turned into a default pointing at nothing.
            .entity(
                Entity::new("#phantom", "FormalParameter")
                    .set("name", "phantom")
                    .reference("workExample", "phantom.csv"),
            )
            .entity(Entity::new("phantom.csv", "File"))
            // A `PropertyValue` `workExample` (a recorded scalar, not a file) -- out of scope
            // for this, skipped rather than misread as a path.
            .entity(
                Entity::new("#scalar", "FormalParameter")
                    .set("name", "scalar")
                    .reference("workExample", "#scalar-value"),
            )
            .entity(Entity::new("#scalar-value", "PropertyValue").set("value", "42"))
            .build();

        let dir = tempfile::tempdir().unwrap();
        crate_.write_directory(dir.path()).unwrap();
        std::fs::write(dir.path().join("main.cwl"), "cwlVersion: v1.2").unwrap();
        std::fs::write(dir.path().join("data.csv"), "a,b\n1,2\n").unwrap();

        let resolved = resolve_target(dir.path()).expect("should resolve");

        let data = resolved
            .default_inputs
            .inputs
            .get("data")
            .expect("data default");
        match data {
            DefaultValue::FileOrDirectory(FileOrDirectory::File(file)) => {
                assert!(file.location.as_deref().unwrap().ends_with("data.csv"));
            }
            other => panic!("expected a File default, got {other:?}"),
        }
        assert!(!resolved.default_inputs.inputs.contains_key("phantom"));
        assert!(!resolved.default_inputs.inputs.contains_key("scalar"));
    }
}
