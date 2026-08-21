//! Running a freshly parsed tool once, locally, to learn what it actually produces.
//!
//! A command line says nothing about its outputs, so the only reliable way to fill in a
//! tool's `outputs` is to run it and diff the working tree.

use super::{ToolCreationOptions};
use crate::{
    authoring::AuthoringResult,
    paths::TrustedPathExt,
    repository::{self, Repository},
};
use anyhow::Context as _;
use commonwl::{
    OneOrMany,
    documents::{CWLDocument, CommandLineTool},
    engine::{
        ContainerEngine, InputObject, LocalBackend, create_execution_request_from_document, execute,
    },
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
use std::{env, fs::remove_file, path::Path, sync::Arc};

use tokio_util::sync::CancellationToken;

/// Executes `cwl` once in `project_root` and reports the files it created, as paths relative
/// to that root.
///
/// `pre_existing` is what the working tree already reported as modified beforehand; anything
/// modified afterwards that isn't in that list is attributed to the run. Honours
/// `options.cleanup` (delete what the run produced) and `options.commit` (stage it).
pub(super) async fn run_and_collect_files(
    cwl: &mut CommandLineTool,
    project_root: &Path,
    options: &ToolCreationOptions,
    repo: &Repository,
    pre_existing: &[String],
) -> AuthoringResult<Vec<String>> {
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
    execute(backend, &job, CancellationToken::new()).await?;

    // Handle modified files
    let mut files = repository::get_modified_files(repo)?;
    files.retain(|f| !pre_existing.contains(f));

    if options.cleanup {
        for file in &files {
            remove_file(project_root.join_trusted_checked(file)?)
                .with_context(|| format!("Failed to remove {file}"))?;
        }
    } else if options.commit {
        for file in &files {
            let path = project_root.join_trusted_checked(file)?; //existence is implied!
            if path.is_dir() {
                repository::stage_dir(repo, &path)?;
            } else {
                repository::stage_file(repo, &path)?;
            }
        }
    }

    Ok(files)
}

/// Globs everything the run leaves behind, minus the files we staged into the working
/// directory ourselves.
fn catch_all_output(cwl: &CommandLineTool) -> CommandOutputParameter {
    CommandOutputParameter::builder()
        .id("catch_all")
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
    p.as_ref()
        .components()
        .next()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .unwrap_or_default()
}
