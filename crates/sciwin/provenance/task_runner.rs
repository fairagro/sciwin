//! The local-execution adapter.

use crate::execution::{ExecutionTiming, RunId, TaskRunner, WorkflowRunner};
use crate::project::config::WorkflowConfig;
use crate::provenance::{
    ProvenanceError, ProvenanceResult, Written,
    builder::build_validated,
    inputs::{CrateInputs, Engine, RunRecord, StepRun},
    write_crate,
};
use chrono::{DateTime, Utc};
use commonwl::{
    Identifiable,
    documents::{CWLDocument, StringOrDocument},
    engine::TaskBackend,
    packed::PackedCWL,
};
use rocrate::validate::Validation;
use std::{collections::HashMap, path::Path};

const ENGINE_NAME: &str = "commonwl";

/// Resolves `entry` into a `PackedCWL` for `WorkflowGraph::from_packed`.
///
/// `commonwl` has no packer for a non-packed, multi-file project (steps referencing other `.cwl`
/// files by relative path) -- if `entry` already has a `$graph`, this is just
/// `packed_from_str`; otherwise it loads each externally-referenced tool and assembles the
/// graph itself, giving each document an id it doesn't already have:
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
/// already resolves those directly, packed or not.
///
/// # Errors
/// `entry` or a referenced tool file does not exist or does not parse.
pub fn pack_project(entry: &Path) -> ProvenanceResult<PackedCWL> {
    let contents = std::fs::read_to_string(entry)?;
    if contents.contains("$graph") {
        return Ok(commonwl::packed_from_str(&contents)?);
    }

    let mut workflow_doc = commonwl::from_str(&contents)?;
    let base_dir = entry.parent().unwrap_or(Path::new("."));
    let mut graph = Vec::new();

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
                let mut tool_doc = commonwl::load_cwl_file(base_dir.join(path), true)?;
                tool_doc.set_id(path);
                qualify_tool_ports(&mut tool_doc, path);
                graph.push(tool_doc);
            }
        }
    }

    if workflow_doc.get_id().is_none() {
        workflow_doc.set_id("#main");
    }
    let cwl_version = workflow_doc.cwl_version().cloned();
    graph.push(workflow_doc);

    Ok(PackedCWL {
        graph,
        cwl_version,
        extension_fields: HashMap::new(),
    })
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

/// Builds and writes the RO-Crate for a finished local run. `entry` must be the same CWL file
/// `run_id` was [`submit`](crate::execution::WorkflowRunner::submit)ted with -- `TaskRunner`
/// doesn't keep it around itself.
///
/// The crate's payload files (the workflow's own `.cwl` files, its inputs and outputs) are not
/// copied in yet, even though they're all sitting on the same machine this runs on -- resolving
/// each crate-relative file name back to a filesystem path is follow-up work. Every one of them
/// shows up in [`Written::missing`] instead of silently being absent.
///
/// # Errors
/// The run has not produced a result yet (still running, or errored before producing one --
/// see [`crate::execution::RunnerError`] for the latter), `entry` doesn't resolve
/// (see [`pack_project`]), or [`write_crate`] fails.
pub async fn export<T: TaskBackend>(
    runner: &TaskRunner<T>,
    run_id: &RunId,
    entry: &Path,
    metadata: WorkflowConfig,
    directory: &Path,
    date_published: DateTime<Utc>,
) -> ProvenanceResult<(Written, Validation)> {
    let Some(timing) = runner.execution_timing(run_id)? else {
        let status = runner.status(run_id).await?;
        return Err(ProvenanceError::NotFinished {
            run: run_id.clone(),
            status,
        });
    };

    let workflow = pack_project(entry)?;
    let run = fold_run_record(&timing);
    let workflow_file = entry
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("workflow.json")
        .to_string();

    let inputs = CrateInputs::builder()
        .workflow(workflow)
        .workflow_file(workflow_file)
        .metadata(metadata)
        .run(run)
        .date_published(date_published)
        .build();

    let (crate_, validation) = build_validated(&inputs)?;
    let written = write_crate(
        &crate_,
        &inputs.workflow,
        &inputs.workflow_file,
        directory,
        &HashMap::new(),
    )?;

    Ok((written, validation))
}

#[cfg(test)]
mod tests {
    use super::*;
    use commonwl::engine::StepTiming;
    use crate::provenance::graph::WorkflowGraph;
    use chrono::NaiveDate;

    fn hello_world_main() -> std::path::PathBuf {
        std::path::PathBuf::from(concat!(
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
        let path = std::path::PathBuf::from(concat!(
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
