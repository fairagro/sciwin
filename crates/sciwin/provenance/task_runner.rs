//! The local-execution adapter. [`pack_document`] and [`fold_run_record`] are pure -- offline
//! testable against real `testdata` fixtures, no running engine needed. [`export`] is the only
//! `async` piece, mirroring `provenance::reana_runner`'s shape.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use chrono::{DateTime, Utc};
use commonwl::{
    Identifiable,
    documents::{CWLDocument, StringOrDocument},
    engine::TaskBackend,
    packed::PackedCWL,
};
use rocrate::validate::Validation;

use crate::execution::{ExecutionTiming, RunId, TaskRunner, WorkflowRunner};
use crate::project::config::WorkflowConfig;
use crate::provenance::{
    ProvenanceError, ProvenanceResult, Written,
    builder::build_validated,
    inputs::{CrateInputs, Engine, RunRecord, StepRun, WorkflowLayout},
    write_crate,
};

const ENGINE_NAME: &str = "commonwl";

/// How [`export`] represents the workflow it ran as crate payload -- see
/// [`crate::provenance::inputs::WorkflowLayout`], which this maps onto once the real files
/// involved are known.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrateLayout {
    /// One `<stem>.packed.cwl` JSON file holds the whole graph.
    Packed,
    /// The original per-file CWL project, still directly re-executable.
    Files,
}

/// Resolves a loaded [`CWLDocument`] into a [`PackedCWL`] for [`crate::provenance::graph::WorkflowGraph::from_packed`],
/// plus the real filesystem path behind every document involved (the workflow itself, keyed by
/// its own id, and each externally-referenced tool, keyed by its `run:` path) -- what
/// [`CrateLayout::Files`] needs to write the original project back out untouched.
///
/// `commonwl` has no packer for a non-packed, multi-file project (steps referencing other `.cwl`
/// files by relative path) -- this loads each one and gives it an id it doesn't already have:
///
/// - a tool gets its `run:` path as its id (e.g. `"../calculation/calculation.cwl"`), and that
///   same prefix on each of its own input/output ids (`"../calculation/calculation.cwl/population"`)
///   -- matching the shape a real packer (cwltool's) produces, which is what
///   `WorkflowGraph`'s connection resolver and `provenance::builder`'s entity ids expect.
/// - the workflow gets `"#main"` if it has no id of its own.
/// - each step's shorthand `in: - id: <port>` (CWL's local-name form, not workflow-qualified)
///   becomes `"<step id>/<port>"`, which is what the same resolver expects on the consuming end
///   of a connection.
///
/// Steps whose `run` is an inline document need none of this -- `WorkflowGraph::from_packed`
/// already resolves those directly, whether `document` came from a packed source or not.
///
/// # Errors
/// A referenced tool file does not exist or does not parse.
pub fn pack_document(
    document: CWLDocument,
    entry_path: &Path,
) -> ProvenanceResult<(PackedCWL, HashMap<String, PathBuf>)> {
    let mut workflow_doc = document;
    let base_dir = entry_path.parent().unwrap_or(Path::new("."));
    let mut graph = Vec::new();
    let mut doc_paths = HashMap::new();

    if let CWLDocument::Workflow(workflow) = &mut workflow_doc {
        for step in &mut workflow.steps {
            let step_id = step.id.clone().unwrap_or_default();
            for step_in in &mut step.r#in {
                if let Some(id) = &step_in.id
                    && !id.contains('/')
                {
                    step_in.id = Some(format!("{step_id}/{id}"));
                }
            }

            if let StringOrDocument::String(path) = &step.run {
                let tool_path = base_dir.join(path);
                let mut tool_doc = commonwl::load_cwl_file(&tool_path, true)?;
                tool_doc.set_id(path);
                qualify_tool_ports(&mut tool_doc, path);
                doc_paths.insert(path.clone(), tool_path);
                graph.push(tool_doc);
            }
        }
    }

    if workflow_doc.get_id().is_none() {
        workflow_doc.set_id("#main");
    }
    let workflow_id = workflow_doc.get_id().cloned().unwrap_or_default();
    doc_paths.insert(workflow_id, entry_path.to_path_buf());

    let cwl_version = workflow_doc.cwl_version().cloned();
    graph.push(workflow_doc);

    Ok((
        PackedCWL {
            graph,
            cwl_version,
            extension_fields: HashMap::new(),
        },
        doc_paths,
    ))
}

/// [`pack_document`], loading `entry` from disk first. A convenience for callers that only have
/// a path and no live [`TaskRunner`] to pull an already-loaded document from (`export` uses
/// [`TaskRunner::specification`] instead, to describe exactly what ran rather than whatever is
/// at `entry` by the time this is called).
///
/// # Errors
/// `entry` does not exist or does not parse; see [`pack_document`].
pub fn pack_project(entry: &Path) -> ProvenanceResult<PackedCWL> {
    let contents = std::fs::read_to_string(entry)?;
    if contents.contains("$graph") {
        return Ok(commonwl::packed_from_str(&contents)?);
    }

    let document = commonwl::from_str(&contents)?;
    let (packed, _) = pack_document(document, entry)?;
    Ok(packed)
}

/// Prefixes a tool's own input/output ids with `tool_id`, matching how a real packer qualifies
/// them (`"population"` -> `"<tool_id>/population"`).
fn qualify_tool_ports(tool_doc: &mut CWLDocument, tool_id: &str) {
    let CWLDocument::CommandLineTool(tool) = tool_doc else {
        return;
    };
    for input in &mut tool.inputs {
        if let Some(id) = &input.id {
            input.id = Some(format!("{tool_id}/{id}"));
        }
    }
    for output in &mut tool.outputs {
        if let Some(id) = &output.id {
            output.id = Some(format!("{tool_id}/{id}"));
        }
    }
}

/// Folds a local run's timing into a [`RunRecord`]. Pure.
#[must_use]
pub fn fold_run_record(timing: &ExecutionTiming) -> RunRecord {
    let steps = timing
        .step_timings
        .iter()
        .map(|step| {
            (
                step.step_id.clone(),
                StepRun {
                    started_at: step.started_at.map(|t| t.and_utc()),
                    ended_at: step.finished_at.map(|t| t.and_utc()),
                    // The packed doc's own declared image is what ran -- unlike REANA, there's
                    // no deployment step locally where the actual image could differ from what
                    // was declared, so `builder`'s fallback to the graph's own image is enough.
                    container_image: None,
                },
            )
        })
        .collect();

    RunRecord {
        engine: Engine {
            name: ENGINE_NAME.to_string(),
            version: None,
        },
        started_at: timing.started_at.map(|t| t.and_utc()),
        ended_at: timing.finished_at.map(|t| t.and_utc()),
        steps,
    }
}

/// Builds and writes the RO-Crate for a finished local run, using exactly the document and path
/// `run_id` was submitted with (see [`TaskRunner::specification`]) -- not a fresh reload, which
/// could disagree with what actually ran if the file changed on disk since.
///
/// # Errors
/// The run has not produced a result yet (still running, or errored before producing one --
/// see [`crate::execution::RunnerError`] for the latter), the specification doesn't resolve
/// (see [`pack_document`]), or [`write_crate`] fails.
pub async fn export<T: TaskBackend>(
    runner: &TaskRunner<T>,
    run_id: &RunId,
    metadata: WorkflowConfig,
    directory: &Path,
    date_published: DateTime<Utc>,
    layout: CrateLayout,
) -> ProvenanceResult<(Written, Validation)> {
    let Some(timing) = runner.execution_timing(run_id)? else {
        let status = runner.status(run_id).await?;
        return Err(ProvenanceError::NotFinished {
            run: run_id.clone(),
            status,
        });
    };

    let spec = runner.specification(run_id)?;
    let (packed, doc_paths) = pack_document(spec.document, &spec.path)?;
    let run = fold_run_record(&timing);

    let (crate_layout, packed_workflow, sources) = match layout {
        CrateLayout::Packed => {
            let stem = spec
                .path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("workflow");
            let file_name = format!("{stem}.packed.cwl");
            (
                WorkflowLayout::Packed {
                    file_name: file_name.clone(),
                },
                Some(file_name),
                HashMap::new(),
            )
        }
        CrateLayout::Files => {
            let file_names: HashMap<String, String> = doc_paths
                .iter()
                .map(|(doc_id, path)| (doc_id.clone(), file_name_of(path)))
                .collect();
            let sources: HashMap<String, PathBuf> = doc_paths
                .into_values()
                .map(|path| (file_name_of(&path), path))
                .collect();
            (WorkflowLayout::Files { file_names }, None, sources)
        }
    };

    let inputs = CrateInputs::builder()
        .workflow(packed)
        .layout(crate_layout)
        .metadata(metadata)
        .run(run)
        .date_published(date_published)
        .build();

    let (crate_, validation) = build_validated(&inputs)?;
    let written = write_crate(
        &crate_,
        directory,
        packed_workflow
            .as_deref()
            .map(|file_name| (file_name, &inputs.workflow)),
        &sources,
    )?;

    Ok((written, validation))
}

fn file_name_of(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provenance::graph::WorkflowGraph;
    use chrono::NaiveDate;
    use commonwl::engine::StepTiming;

    fn hello_world_main() -> PathBuf {
        PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../testdata/hello_world/workflows/main/main.cwl"
        ))
    }

    #[test]
    fn test_pack_project_resolves_external_steps_and_connections() {
        let packed = pack_project(&hello_world_main()).unwrap();

        // main workflow + calculation.cwl + plot.cwl.
        assert_eq!(packed.graph.len(), 3);

        let graph = WorkflowGraph::from_packed(&packed).unwrap();
        assert_eq!(graph.steps.len(), 2);

        let calculation = graph.steps.iter().find(|s| s.id == "calculation").unwrap();
        assert_eq!(calculation.run, "../calculation/calculation.cwl");
        assert_eq!(
            calculation.container_image.as_deref(),
            Some("pandas/pandas:pip-all")
        );

        let plot = graph.steps.iter().find(|s| s.id == "plot").unwrap();
        assert_eq!(plot.run, "../plot/plot.cwl");
        assert_eq!(
            plot.container_image.as_deref(),
            Some("sciwin/python-datascience")
        );

        let mut connections = graph.connections.clone();
        connections.sort();
        let mut expected = vec![
            (
                "../calculation/calculation.cwl/results".to_string(),
                "../plot/plot.cwl/results".to_string(),
            ),
            ("../plot/plot.cwl/o_results".to_string(), "out".to_string()),
            (
                "population".to_string(),
                "../calculation/calculation.cwl/population".to_string(),
            ),
            (
                "speakers".to_string(),
                "../calculation/calculation.cwl/speakers".to_string(),
            ),
        ];
        expected.sort();
        assert_eq!(connections, expected);
    }

    #[test]
    fn test_pack_project_already_packed() {
        let path = PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../testdata/rocrate/workflow.json"
        ));
        let raw: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let contents = serde_json::to_string(&raw["workflow"]["specification"]).unwrap();

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("packed.json");
        std::fs::write(&file, contents).unwrap();

        let packed = pack_project(&file).unwrap();
        assert_eq!(packed.graph.len(), 3);
    }

    #[test]
    fn test_pack_document_reports_real_file_paths() {
        let entry = hello_world_main();
        let contents = std::fs::read_to_string(&entry).unwrap();
        let document = commonwl::from_str(&contents).unwrap();

        let (packed, doc_paths) = pack_document(document, &entry).unwrap();
        assert_eq!(packed.graph.len(), 3);

        assert_eq!(doc_paths.get("#main"), Some(&entry));
        let calculation_path = doc_paths.get("../calculation/calculation.cwl").unwrap();
        assert!(calculation_path.ends_with("calculation/calculation.cwl"));
        assert!(calculation_path.exists());
        let plot_path = doc_paths.get("../plot/plot.cwl").unwrap();
        assert!(plot_path.ends_with("plot/plot.cwl"));
        assert!(plot_path.exists());

        // What `export`'s `CrateLayout::Files` branch derives from this map.
        let file_names: HashMap<String, String> = doc_paths
            .iter()
            .map(|(doc_id, path)| (doc_id.clone(), file_name_of(path)))
            .collect();
        assert_eq!(file_names.get("#main").unwrap(), "main.cwl");
        assert_eq!(
            file_names.get("../calculation/calculation.cwl").unwrap(),
            "calculation.cwl"
        );
    }

    #[test]
    fn test_fold_run_record() {
        let timing = ExecutionTiming {
            started_at: Some(
                NaiveDate::from_ymd_opt(2026, 7, 23)
                    .unwrap()
                    .and_hms_opt(12, 21, 26)
                    .unwrap(),
            ),
            finished_at: Some(
                NaiveDate::from_ymd_opt(2026, 7, 23)
                    .unwrap()
                    .and_hms_opt(12, 21, 37)
                    .unwrap(),
            ),
            step_timings: vec![StepTiming::new(
                "calculation",
                Some(
                    NaiveDate::from_ymd_opt(2026, 7, 23)
                        .unwrap()
                        .and_hms_opt(12, 21, 26)
                        .unwrap(),
                ),
                Some(
                    NaiveDate::from_ymd_opt(2026, 7, 23)
                        .unwrap()
                        .and_hms_opt(12, 21, 30)
                        .unwrap(),
                ),
            )],
        };

        let run = fold_run_record(&timing);

        assert_eq!(run.engine.name, "commonwl");
        assert!(run.started_at.is_some());
        assert!(run.ended_at.is_some());
        assert_eq!(run.steps.len(), 1);
        assert!(run.steps.get("calculation").unwrap().started_at.is_some());
    }
}
