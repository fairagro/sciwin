use commonwl::packed::PackedCWL;
use miette::Diagnostic;
use rocrate::RoCrate;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};
use thiserror::Error;

pub mod builder;
pub mod graph;
pub mod inputs;
pub mod reana_runner;
pub mod task_runner;

/// Result alias for this module. See [`crate::Result`] for code spanning several modules.
pub type ProvenanceResult<T> = Result<T, ProvenanceError>;

/// Anything RO-Crate provenance recording/export can fail with.
#[derive(Error, Diagnostic, Debug)]
pub enum ProvenanceError {
    #[error("the workflow specification has no Workflow in its $graph")]
    #[diagnostic(code = "sciwin::provenance::NoWorkflow")]
    NoWorkflow,

    #[error("step `{step}` runs `{run}`, which is not in the packed graph")]
    #[diagnostic(code = "sciwin::provenance::UnresolvedStep")]
    UnresolvedStep { step: String, run: String },

    #[error("workflow `{run}` is {status:?}; only a finished run can be exported")]
    #[diagnostic(code = "sciwin::provenance::NotFinished")]
    NotFinished {
        run: crate::execution::RunId,
        status: crate::execution::RunStatus,
    },

    #[error(transparent)]
    #[diagnostic(transparent)]
    Invalid(#[from] rocrate::validate::InvalidCrate),

    #[error(transparent)]
    #[diagnostic(transparent)]
    CrateIo(#[from] rocrate::io::Error),

    #[error(transparent)]
    #[diagnostic(transparent)]
    Reana(#[from] reana::error::ClientError),

    #[error(transparent)]
    #[diagnostic(transparent)]
    Project(#[from] crate::project::ProjectError),

    #[error(transparent)]
    #[diagnostic(transparent)]
    Runner(#[from] crate::execution::RunnerError),

    #[error(transparent)]
    #[diagnostic(transparent)]
    Commonwl(#[from] commonwl::Error),

    #[error(transparent)]
    #[diagnostic(code = "std::io::Error")]
    IO(#[from] std::io::Error),

    #[error(transparent)]
    #[diagnostic(code = "serde_json::Error")]
    JSON(#[from] serde_json::Error),
}

/// What [`write_crate`] actually did, for a caller (the CLI) to report or act on.
#[derive(Debug, Clone)]
pub struct Written {
    /// Path to the written `ro-crate-metadata.json`.
    pub metadata: PathBuf,
    /// Crate-relative names of every `sources` entry that got copied in.
    pub copied: Vec<String>,
    /// Crate-relative names of local parts the crate still doesn't have on disk -- neither
    /// written by this call nor already present in `directory`. A backend (the REANA adapter,
    /// say) still needs to fetch these.
    pub missing: Vec<String>,
}

/// Packages a built [`RoCrate`] onto disk: the metadata, optionally the packed workflow itself
///
/// `packed_workflow` is `Some((file_name, packed))` for [`crate::provenance::inputs::WorkflowLayout::Packed`]
/// -- write the one packed JSON file -- and `None` for
/// [`crate::provenance::inputs::WorkflowLayout::Files`], where every crate file (including the
/// main workflow's own) is real bytes already on disk, passed in through `sources` instead.
///
/// # Errors
/// The directory is not writable, or `packed` fails to serialize.
pub fn write_crate(
    crate_: &RoCrate,
    directory: &Path,
    packed_workflow: Option<(&str, &PackedCWL)>,
    sources: &HashMap<String, PathBuf>,
) -> ProvenanceResult<Written> {
    let metadata = crate_.write_directory(directory)?;

    if let Some((file_name, packed)) = packed_workflow {
        let packed_json = serde_json::to_string_pretty(packed)?;
        std::fs::write(directory.join(file_name), packed_json)?;
    }

    let mut copied = Vec::new();
    for (name, source) in sources {
        std::fs::copy(source, directory.join(name))?;
        copied.push(name.clone());
    }

    let missing = crate_
        .missing_parts(directory)
        .into_iter()
        .map(str::to_string)
        .collect();

    Ok(Written {
        metadata,
        copied,
        missing,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::config::WorkflowConfig;
    use crate::provenance::{builder::build_crate, inputs::RunRecord};
    use chrono::Utc;
    use std::fs;

    fn fixture_packed() -> PackedCWL {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../testdata/rocrate/workflow.json"
        );
        let raw: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
        serde_json::from_value(raw["workflow"]["specification"].clone()).unwrap()
    }

    fn fixture_crate(packed: &PackedCWL) -> RoCrate {
        let inputs = crate::provenance::inputs::CrateInputs::builder()
            .workflow(packed.clone())
            .metadata(WorkflowConfig {
                name: "hello_s4n".to_string(),
                ..Default::default()
            })
            .run(RunRecord::default())
            .date_published(Utc::now())
            .build();
        build_crate(&inputs).unwrap()
    }

    #[test]
    fn test_write_crate_creates_metadata_and_workflow_file() {
        let packed = fixture_packed();
        let crate_ = fixture_crate(&packed);

        let dir = tempfile::tempdir().unwrap();
        let written = write_crate(
            &crate_,
            dir.path(),
            Some(("workflow.json", &packed)),
            &HashMap::new(),
        )
        .unwrap();

        assert!(written.metadata.exists());
        assert!(dir.path().join("workflow.json").exists());
        assert!(written.copied.is_empty());
        // Nothing was in `sources`, so every data file the crate references is still missing.
        assert!(!written.missing.is_empty());

        let read_back = RoCrate::from_directory(dir.path()).unwrap();
        assert_eq!(
            read_back.main_entity().unwrap().id,
            crate_.main_entity().unwrap().id
        );
    }

    #[test]
    fn test_write_crate_copies_sources_and_reports_none_missing() {
        let packed = fixture_packed();
        let crate_ = fixture_crate(&packed);

        let dir = tempfile::tempdir().unwrap();
        let mut sources = HashMap::new();
        for name in [
            "population.csv",
            "speakers_revised.csv",
            "results.csv",
            "results.svg",
        ] {
            let source_path = dir.path().join(format!("src-{name}"));
            fs::write(&source_path, "x").unwrap();
            sources.insert(name.to_string(), source_path);
        }

        let written = write_crate(
            &crate_,
            dir.path(),
            Some(("workflow.json", &packed)),
            &sources,
        )
        .unwrap();

        assert_eq!(written.copied.len(), 4);
        assert!(written.missing.is_empty());
        for name in [
            "population.csv",
            "speakers_revised.csv",
            "results.csv",
            "results.svg",
        ] {
            assert!(dir.path().join(name).exists());
        }
    }

    #[test]
    fn test_write_crate_files_layout_writes_no_packed_json() {
        let packed = fixture_packed();
        let inputs = crate::provenance::inputs::CrateInputs::builder()
            .workflow(packed)
            .layout(crate::provenance::inputs::WorkflowLayout::Files {
                file_names: HashMap::from([("#main".to_string(), "main.cwl".to_string())]),
            })
            .metadata(WorkflowConfig {
                name: "hello_s4n".to_string(),
                ..Default::default()
            })
            .run(RunRecord::default())
            .date_published(Utc::now())
            .build();
        let crate_ = crate::provenance::builder::build_crate(&inputs).unwrap();

        let dir = tempfile::tempdir().unwrap();
        let main_source = dir.path().join("src-main.cwl");
        fs::write(&main_source, "cwlVersion: v1.2").unwrap();
        let sources = HashMap::from([("main.cwl".to_string(), main_source)]);

        let written = write_crate(&crate_, dir.path(), None, &sources).unwrap();

        assert!(written.metadata.exists());
        assert_eq!(written.copied, vec!["main.cwl".to_string()]);
        assert!(dir.path().join("main.cwl").exists());
        // No packed JSON written under any name.
        assert!(!dir.path().join("workflow.json").exists());
        assert_eq!(
            fs::read_to_string(dir.path().join("main.cwl")).unwrap(),
            "cwlVersion: v1.2"
        );
    }
}
