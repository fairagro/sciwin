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
    inputs::DefaultValue,
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
pub async fn export(
    runner: &TaskRunner,
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
    let base_dir = spec.path.parent().unwrap_or(Path::new(".")).to_path_buf();
    let (packed, doc_paths) = pack_document(spec.document, &spec.path)?;
    let run = fold_run_record(&timing);

    // Data payload: every input's declared default, plus the run's actual final outputs.
    // Genuine step-to-step intermediates (a file produced and consumed entirely within the run,
    // never a workflow-level output) aren't resolvable from what `TaskBackend` exposes today --
    // they still end up in `Written::missing`, same as a REANA download that 404s.
    let mut sources = resolve_input_sources(&packed, &base_dir);
    if let Some(outputs) = runner.outputs(run_id, None).await? {
        sources.extend(resolve_output_sources(&outputs, &base_dir));
    }

    let (crate_layout, packed_workflow) = match layout {
        CrateLayout::Packed => {
            let stem = spec
                .path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("workflow");
            let file_name = format!("{stem}.packed.cwl");
            sources.extend(resolve_include_sources(&packed, &doc_paths, None));
            (
                WorkflowLayout::Packed {
                    file_name: file_name.clone(),
                },
                Some(file_name),
            )
        }
        CrateLayout::Files => {
            let file_names = crate_relative_names(&doc_paths);
            sources.extend(
                doc_paths
                    .iter()
                    .map(|(doc_id, path)| (file_names[doc_id].clone(), path.clone())),
            );
            sources.extend(resolve_include_sources(
                &packed,
                &doc_paths,
                Some(&file_names),
            ));
            (WorkflowLayout::Files { file_names }, None)
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

fn crate_relative_names(doc_paths: &HashMap<String, PathBuf>) -> HashMap<String, String> {
    // `pack_document` builds a tool's path via a plain `base_dir.join("../calculation/x.cwl")`,
    // leaving the literal `..` component in place
    let canonical: HashMap<&String, PathBuf> = doc_paths
        .iter()
        .map(|(doc_id, path)| {
            (
                doc_id,
                dunce::canonicalize(path).unwrap_or_else(|_| path.clone()),
            )
        })
        .collect();
    let root = common_ancestor(canonical.values());
    canonical
        .into_iter()
        .map(|(doc_id, path)| {
            let relative = path.strip_prefix(&root).unwrap_or(&path);
            (doc_id.clone(), to_slash_string(relative))
        })
        .collect()
}

/// The deepest directory every path in `paths` has in common.
fn common_ancestor<'a>(paths: impl Iterator<Item = &'a PathBuf>) -> PathBuf {
    let mut ancestor: Option<Vec<std::ffi::OsString>> = None;
    for path in paths {
        let dir = path.parent().unwrap_or(Path::new("."));
        let components: Vec<_> = dir
            .components()
            .map(|c| c.as_os_str().to_os_string())
            .collect();
        ancestor = Some(match ancestor {
            None => components,
            Some(prefix) => {
                let shared = prefix
                    .iter()
                    .zip(components.iter())
                    .take_while(|(a, b)| a == b)
                    .count();
                prefix.into_iter().take(shared).collect()
            }
        });
    }
    ancestor.unwrap_or_default().into_iter().collect()
}

/// A relative path as a crate id: `/`-joined regardless of host path separator, since RO-Crate
/// ids are URIs, not filesystem paths.
fn to_slash_string(path: &Path) -> String {
    path.components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

fn resolve_include_sources(
    packed: &PackedCWL,
    doc_paths: &HashMap<String, PathBuf>,
    file_names: Option<&HashMap<String, String>>,
) -> HashMap<String, PathBuf> {
    use commonwl::requirements::{
        InitialWorkDirRequirement, ListingItems, StringOrInclude, WorkDirItems,
    };

    let mut sources = HashMap::new();
    for doc in &packed.graph {
        let Some(doc_id) = doc.get_id() else { continue };
        let Some(real_path) = doc_paths.get(doc_id) else {
            continue;
        };
        let real_dir = real_path.parent().unwrap_or(Path::new("."));
        let crate_dir = file_names
            .and_then(|names| names.get(doc_id))
            .map(|name| {
                Path::new(name)
                    .parent()
                    .unwrap_or(Path::new(""))
                    .to_path_buf()
            })
            .unwrap_or_default();

        let Some(iwdr) = doc.get_requirement::<InitialWorkDirRequirement>() else {
            continue;
        };
        let WorkDirItems::ListingItems(items) = &iwdr.listing else {
            continue;
        };
        for item in items {
            let ListingItems::Dirent(dirent) = item else {
                continue;
            };
            let StringOrInclude::Include(include) = &dirent.entry else {
                continue;
            };
            let source = real_dir.join(&include.include);
            if source.exists() {
                let crate_name = to_slash_string(&crate_dir.join(&include.include));
                sources.insert(crate_name, source);
            }
        }
    }
    sources
}

/// Every input default across the whole packed graph (the workflow's own, and each tool's) that
/// names a real file on disk, keyed by crate-relative file name. `CWLDocument::get_inputs()` is
/// the one accessor that works the same way for a `Workflow`'s inputs and a
/// `CommandLineTool`'s -- exactly the two kinds of document `pack_document` ever produces.
fn resolve_input_sources(packed: &PackedCWL, base_dir: &Path) -> HashMap<String, PathBuf> {
    let mut sources = HashMap::new();
    for doc in &packed.graph {
        for input in doc.get_inputs() {
            let Some(file) = input.default.as_ref().and_then(DefaultValue::as_file) else {
                continue;
            };
            let Some(path) = resolve_file_path(base_dir, file) else {
                continue;
            };
            if path.exists() {
                sources.insert(file_name_of(&path), path);
            }
        }
    }
    sources
}

/// The run's own final outputs (from [`WorkflowRunner::outputs`]) that name a real file,
/// keyed the same way as [`resolve_input_sources`].
fn resolve_output_sources(
    outputs: &HashMap<String, DefaultValue>,
    base_dir: &Path,
) -> HashMap<String, PathBuf> {
    let mut sources = HashMap::new();
    for value in outputs.values() {
        let Some(file) = value.as_file() else {
            continue;
        };
        let Some(path) = resolve_file_path(base_dir, file) else {
            continue;
        };
        if path.exists() {
            sources.insert(file_name_of(&path), path);
        }
    }
    sources
}

/// A `File`'s `path` if it has one, else `location` resolved against `base_dir` (stripping a
/// `file://` scheme first, and left alone if already absolute).
fn resolve_file_path(base_dir: &Path, file: &commonwl::files::File) -> Option<PathBuf> {
    if let Some(path) = &file.path {
        return Some(PathBuf::from(path));
    }
    let location = file.location.as_deref()?;
    let location = location.strip_prefix("file://").unwrap_or(location);
    let candidate = PathBuf::from(location);
    Some(if candidate.is_absolute() {
        candidate
    } else {
        base_dir.join(candidate)
    })
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

        let file_names = crate_relative_names(&doc_paths);
        assert_eq!(file_names.get("#main").unwrap(), "main/main.cwl");
        assert_eq!(
            file_names.get("../calculation/calculation.cwl").unwrap(),
            "calculation/calculation.cwl"
        );
        assert_eq!(file_names.get("../plot/plot.cwl").unwrap(), "plot/plot.cwl");
    }

    #[test]
    fn test_crate_relative_names_collapses_single_document_to_bare_filename() {
        let entry = PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../testdata/echo.cwl"
        ));
        let doc_paths = HashMap::from([("#main".to_string(), entry)]);

        let file_names = crate_relative_names(&doc_paths);

        assert_eq!(file_names.get("#main").unwrap(), "echo.cwl");
    }

    #[test]
    fn test_resolve_include_sources_finds_tool_scripts() {
        let entry = hello_world_main();
        let contents = std::fs::read_to_string(&entry).unwrap();
        let document = commonwl::from_str(&contents).unwrap();
        let (packed, doc_paths) = pack_document(document, &entry).unwrap();
        let file_names = crate_relative_names(&doc_paths);

        let sources = resolve_include_sources(&packed, &doc_paths, Some(&file_names));

        // Each tool's own embedded script lands alongside its (now-nested) CWL file, not at the
        // crate root -- matching where `$include: calculation.py` will look for it once staged.
        assert!(sources.contains_key("calculation/calculation.py"));
        assert!(sources.contains_key("plot/plot.py"));
        assert!(sources["calculation/calculation.py"].exists());

        let packed_sources = resolve_include_sources(&packed, &doc_paths, None);
        assert!(packed_sources.contains_key("calculation.py"));
        assert!(packed_sources.contains_key("plot.py"));
    }

    #[test]
    fn test_resolve_input_sources_finds_real_data_files() {
        let entry = hello_world_main();
        let contents = std::fs::read_to_string(&entry).unwrap();
        let document = commonwl::from_str(&contents).unwrap();
        let (packed, _) = pack_document(document, &entry).unwrap();
        let base_dir = entry.parent().unwrap();

        let sources = resolve_input_sources(&packed, base_dir);

        let population = sources.get("population.csv").unwrap();
        assert!(population.exists());
        assert!(population.ends_with("data/population.csv"));

        let speakers = sources.get("speakers_revised.csv").unwrap();
        assert!(speakers.exists());
        assert!(speakers.ends_with("data/speakers_revised.csv"));
    }

    #[test]
    fn test_resolve_output_sources_finds_real_files_and_skips_missing() {
        use commonwl::files::{File, FileOrDirectory};

        let dir = tempfile::tempdir().unwrap();
        let real_file = dir.path().join("results.svg");
        std::fs::write(&real_file, "svg").unwrap();

        let mut outputs = HashMap::new();
        outputs.insert(
            "out".to_string(),
            DefaultValue::FileOrDirectory(FileOrDirectory::File(
                File::builder()
                    .path(real_file.to_string_lossy().into_owned())
                    .build(),
            )),
        );
        outputs.insert(
            "gone".to_string(),
            DefaultValue::FileOrDirectory(FileOrDirectory::File(
                File::builder()
                    .path(
                        dir.path()
                            .join("does-not-exist.txt")
                            .to_string_lossy()
                            .into_owned(),
                    )
                    .build(),
            )),
        );

        let sources = resolve_output_sources(&outputs, dir.path());

        assert_eq!(sources.len(), 1);
        assert_eq!(sources.get("results.svg"), Some(&real_file));
        assert!(!sources.contains_key("does-not-exist.txt"));
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
