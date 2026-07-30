//! Turning a shell command into a CWL `CommandLineTool`.
//!
//! The pipeline, in order:
//!
//! 1. [`parser::parse_command_line`] turns the raw tokens into a `CommandLineTool`
//! 2. [`probe`] optionally runs it once to discover which files it produces
//! 3. [`requirements`] attaches container, network, environment and mount requirements
//! 4. [`save`] post-processes, rebases paths on the target location, and serializes

mod probe;
mod requirements;
mod save;

pub use requirements::ContainerInfo;

use crate::{
    authoring::{AuthoringError, AuthoringResult, parser, paths},
    repository::{self, Repository},
};
use commonwl::{documents::CommandLineTool, engine::ContainerEngine};
use std::{
    env,
    path::{Path, PathBuf},
};

#[derive(Default)]
pub struct ToolCreationOptions<'a> {
    pub command: &'a [String],
    pub outputs: &'a [String],
    pub inputs: &'a [String],
    pub no_run: bool,
    pub cleanup: bool,
    pub commit: bool,
    pub clear_defaults: bool,
    pub container: Option<ContainerInfo<'a>>,
    pub enable_network: bool,
    pub mounts: &'a [PathBuf],
    pub env: Option<&'a Path>,
    pub run_container: Option<ContainerEngine>,
    pub output_dir: Option<&'a Path>,
}

/// Builds a `CommandLineTool` from `options.command` and returns it as formatted CWL YAML,
/// optionally writing it to disk.
pub async fn create_tool(
    options: &ToolCreationOptions<'_>,
    name: Option<String>,
    save_to_disk: bool,
) -> AuthoringResult<String> {
    let mut cwl = create_tool_base(options).await?;

    if options.run_container.is_none() {
        requirements::add_tool_requirements(&mut cwl, options)?;
    } else if let Some(container) = &options.container
        && requirements::is_sif_image(container.image)
    {
        //if run_container is some requirements are already set in create_tool_base()
        //just the docker requirements needs to be altered in case of sif file
        requirements::rewrite_sif_container_mut(&mut cwl, container);
    }

    // Finalize CWL
    let base_command = cwl.base_command.as_ref().unwrap();
    let output_dir = options
        .output_dir
        .map(Path::to_path_buf)
        .unwrap_or_else(|| {
            // default as in 1.x
            Path::new(paths::WORKFLOWS_FOLDER)
                .join(paths::derive_tool_name(base_command, name.as_deref()))
        });
    let path = paths::get_qualified_filename(base_command, name.as_deref(), output_dir);
    let yaml = save::finalize_tool(&mut cwl, &path)?;

    if save_to_disk {
        let cwd = env::current_dir()?;
        let repo = Repository::open(&cwd)
            .map_err(|source| AuthoringError::NoRepository { path: cwd, source })?;
        save::save_tool_to_disk(&yaml, &path, &repo, options.commit)?;
    }
    Ok(yaml)
}

/// Parses the command line into a tool and, unless `options.no_run` is set, runs it once to
/// discover its outputs.
async fn create_tool_base(options: &ToolCreationOptions<'_>) -> AuthoringResult<CommandLineTool> {
    let command = options
        .command
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let current_working_dir = env::current_dir()?;

    //check for modified files and fail if there are any
    let repo =
        Repository::open(&current_working_dir).map_err(|source| AuthoringError::NoRepository {
            path: current_working_dir,
            source,
        })?;
    let modified = repository::get_modified_files(&repo)?;

    if !options.no_run && !modified.is_empty() {
        return Err(AuthoringError::DirtyWorkingTree { files: modified });
    }

    //parse command
    let mut cwl = parser::parse_command_line(&command);
    cwl.cwl_version = Some("v1.2".to_string());

    // handle outputs
    if !options.outputs.is_empty() {
        cwl.outputs = parser::get_outputs(options.outputs);
    }

    if !options.inputs.is_empty() {
        parser::add_fixed_inputs(
            &mut cwl,
            &options
                .inputs
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
        )?;
    }

    if !options.no_run {
        let files = probe::run_and_collect_files(&mut cwl, options, &repo, &modified).await?;
        if options.outputs.is_empty() {
            cwl.outputs = parser::get_outputs(&files);
        }
    }

    // Clear defaults if requested
    if options.clear_defaults {
        for input in &mut cwl.inputs {
            input.default = None;
        }
    }
    Ok(cwl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fstest::fstest;
    use std::fs;

    /// A dirty working tree is a state a frontend acts on (offer to commit, or to re-run
    /// with `no_run`), so it must arrive as a matchable variant, not a formatted string.
    #[fstest(repo = true, tokio = true, files = ["../../testdata/input.txt"])]
    pub async fn test_dirty_working_tree_is_a_typed_variant() {
        fs::write("uncommitted.txt", "scratch").unwrap();

        let command = ["echo".to_string(), "hello".to_string()];
        let options = ToolCreationOptions {
            command: &command,
            ..Default::default()
        };

        let err = create_tool(&options, None, false).await.unwrap_err();
        let AuthoringError::DirtyWorkingTree { files } = &err else {
            panic!("expected DirtyWorkingTree, got {err:?}");
        };
        assert!(files.iter().any(|f| f == "uncommitted.txt"));
    }

    /// Same reasoning: a missing repository is a state a frontend offers to fix.
    #[fstest(tokio = true)]
    pub async fn test_missing_repository_is_a_typed_variant() {
        let command = ["echo".to_string(), "hello".to_string()];
        let options = ToolCreationOptions {
            command: &command,
            ..Default::default()
        };

        let err = create_tool(&options, None, false).await.unwrap_err();
        assert!(
            matches!(err, AuthoringError::NoRepository { .. }),
            "expected NoRepository, got {err:?}"
        );
    }
}
