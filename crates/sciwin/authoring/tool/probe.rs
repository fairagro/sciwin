//! Running a freshly parsed tool once, locally, to learn what it actually produces.
//!
//! A command line says nothing about its outputs, so the only reliable way to fill in a
//! tool's `outputs` is to run it and see what lands in its working directory.

use super::ToolCreationOptions;
use crate::{authoring::AuthoringResult, paths::TrustedPathExt};
use anyhow::Context as _;
use commonwl::{
    OneOrMany,
    documents::{CWLDocument, CommandLineTool},
    engine::{
        ContainerEngine, InputObject, LocalBackend, create_execution_request_from_document, execute,
    },
    inputs::DefaultValue,
    outputs::{
        CommandOutputArraySchema, CommandOutputBinding, CommandOutputParameter,
        CommandOutputSchema, CommandOutputType,
    },
    requirements::{
        InitialWorkDirRequirement, InlineJavascriptRequirement, ListingItems, ToolRequirements,
        WorkDirItems,
    },
    storage::{StorageBackend, StoragePath},
    types::CWLType,
};
use serde_json::Value;
use std::{
    env, fs,
    path::{Component, Path, PathBuf},
    sync::Arc,
};
use tracing::warn;
use url::Url;

use tokio_util::sync::CancellationToken;

const CATCH_ALL: &str = "catch_all";

/// A file or directory the trial run produced.
#[derive(Debug, PartialEq)]
pub(super) struct Produced {
    /// Its name in the tool's working directory, what the output's glob matches.
    pub name: String,
    /// Where its copy landed, relative to the project root. The engine never overwrites, so
    /// this differs from `name` when a different file of that name already existed.
    pub path: String,
}

/// Executes `cwl` once against `project_root`, where the run's outputs are copied, and
/// reports what it produced. Honours `options.cleanup` (delete the copies again).
pub(super) async fn run_and_collect_files(
    cwl: &mut CommandLineTool,
    project_root: &Path,
    options: &ToolCreationOptions,
) -> AuthoringResult<Vec<Produced>> {
    let storage = Arc::new(StorageBackend::new());
    let backend = Arc::new(LocalBackend::new(
        options.run_container.unwrap_or(ContainerEngine::Docker),
        storage,
        StoragePath::from_local(&env::temp_dir()),
    ));

    // the probe runs a copy: the catch-all output and the JS requirement it needs exist only
    // to observe the run, and must not end up in the saved tool
    let mut probe_cwl = cwl.clone();
    probe_cwl.outputs.push(catch_all_output(cwl));
    probe_cwl.append_requirement_mut(ToolRequirements::InlineJavascriptRequirement(
        InlineJavascriptRequirement::default(),
    ));

    // `outputs_path` must be explicit: left `None`, the engine falls back to `env::current_dir()`
    let job = create_execution_request_from_document(
        CWLDocument::CommandLineTool(probe_cwl),
        InputObject::default(),
        project_root,
        Some(project_root),
        None,
    )?;
    let result = execute(backend, &job, CancellationToken::new()).await?;
    let produced = produced_files(result.outputs.get(CATCH_ALL), project_root);
    for file in produced.iter().filter(|f| f.name != f.path) {
        warn!(
            "{} already existed, the trial run's copy was saved as {}",
            file.name, file.path
        );
    }

    if options.cleanup {
        for file in &produced {
            let path = project_root.join_trusted_checked(&file.path)?;
            if path.is_dir() {
                fs::remove_dir_all(&path)
            } else {
                fs::remove_file(&path)
            }
            .with_context(|| format!("Failed to remove {}", file.path))?;
        }
    }

    Ok(produced)
}

/// What the catch-all output collected, sorted by name. Its glob only matches the top level
/// of the working directory, so a basename is the path relative to it.
fn produced_files(catch_all: Option<&DefaultValue>, project_root: &Path) -> Vec<Produced> {
    let value = catch_all
        .and_then(|v| serde_json::to_value(v).ok())
        .unwrap_or(Value::Null);
    let entries = match value {
        Value::Array(entries) => entries,
        Value::Null => vec![],
        single => vec![single],
    };
    let canonical_root = dunce::canonicalize(project_root).ok();
    let mut produced: Vec<Produced> = entries
        .iter()
        .filter_map(|entry| {
            let landed = landed_path(entry);
            let name = entry
                .get("basename")
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| Some(landed.as_ref()?.file_name()?.to_str()?.to_string()))?;
            let path = landed
                .as_deref()
                .and_then(|p| {
                    p.strip_prefix(project_root)
                        .ok()
                        .or_else(|| p.strip_prefix(canonical_root.as_ref()?).ok())
                })
                .map_or_else(|| name.clone(), |p| p.to_string_lossy().into_owned());
            Some(Produced { name, path })
        })
        .collect();
    produced.sort_by(|a, b| a.name.cmp(&b.name));
    produced.dedup();
    produced
}

/// The absolute path an output entry's copy was written to: its `path`, else its `location`.
fn landed_path(entry: &Value) -> Option<PathBuf> {
    if let Some(path) = entry.get("path").and_then(Value::as_str) {
        return Some(PathBuf::from(path));
    }
    let location = entry.get("location").and_then(Value::as_str)?;
    match Url::parse(location) {
        Ok(url) => url.to_file_path().ok(),
        Err(_) => Some(PathBuf::from(location)),
    }
}

/// Globs everything the run leaves behind, minus the files we staged into the working
/// directory ourselves.
fn catch_all_output(cwl: &CommandLineTool) -> CommandOutputParameter {
    CommandOutputParameter::builder()
        .id(CATCH_ALL)
        .output_binding(
            CommandOutputBinding::builder()
                .glob(OneOrMany::One("*".to_string()))
                .output_eval(filter_output(cwl)) //if something strange happens: this is the culprit
                .build(),
        )
        .r#type(CommandOutputType::CommandOutputSchema(Box::new(
            CommandOutputSchema::Array(
                CommandOutputArraySchema::builder()
                    .items(OneOrMany::Many(vec![
                        CommandOutputType::CWLType(CWLType::Null),
                        CommandOutputType::CWLType(CWLType::File),
                        CommandOutputType::CWLType(CWLType::Directory),
                    ]))
                    .build(),
            ),
        )))
        .build()
}

/// Builds the JS `outputEval` that drops staged inputs from the catch-all glob.
fn filter_output(cwl: &CommandLineTool) -> String {
    let staged_roots = get_iwdr_roots(cwl);
    // serde_json::to_string escapes quotes/backslashes in root names
    let staged_js = serde_json::to_string(&staged_roots).unwrap_or_else(|_| "[]".to_string());
    format!(
        "${{ var staged = {staged_js}; \
       return self.filter(function(f) {{ \
         return staged.indexOf(f.basename) === -1; \
       }}); \
    }}"
    )
}

/// The first path component of every file staged via `InitialWorkDirRequirement` -- what the
/// glob sees in the output directory.
fn get_iwdr_roots(cwl: &CommandLineTool) -> Vec<String> {
    let Some(iwdr) = cwl.get_requirement::<InitialWorkDirRequirement>() else {
        return vec![];
    };
    let WorkDirItems::ListingItems(items) = &iwdr.listing else {
        return vec![];
    };

    items
        .iter()
        .filter_map(|item| match item {
            ListingItems::Dirent(dirent) => dirent.entryname.as_deref().map(root_path),
            //only Dirents name a staged path; the other variants stage by value
            _ => None,
        })
        .collect()
}

fn root_path(p: impl AsRef<Path>) -> String {
    // `./script.sh` is staged as `script.sh`, so a leading `.` is not its root
    p.as_ref()
        .components()
        .find(|c| !matches!(c, Component::CurDir))
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn produced(name: &str, path: &str) -> Produced {
        Produced {
            name: name.to_string(),
            path: path.to_string(),
        }
    }

    #[test]
    fn produced_files_keeps_the_name_and_where_the_copy_landed() {
        let root = tempfile::tempdir().unwrap();
        let url = |name: &str| {
            Url::from_file_path(root.path().join(name))
                .unwrap()
                .to_string()
        };
        let value = DefaultValue::Any(json!([
            {"class": "File", "basename": "results.csv", "location": url("results_2.csv")},
            {"class": "Directory", "location": url("plots")},
        ]));
        assert_eq!(
            produced_files(Some(&value), root.path()),
            [
                produced("plots", "plots"),
                produced("results.csv", "results_2.csv")
            ]
        );
        assert!(produced_files(None, root.path()).is_empty());
    }

    #[test]
    fn root_path_skips_a_leading_dot() {
        assert_eq!(root_path("./script.sh"), "script.sh");
        assert_eq!(root_path("workflows/calc/calc.py"), "workflows");
    }
}
