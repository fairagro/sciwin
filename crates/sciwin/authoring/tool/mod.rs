//! Turning a shell command into a CWL `CommandLineTool`.
//!
//! [`create_tool`] is the entry point. The pipeline, in order:
//!
//! 1. `parser::parse_command_line` turns the raw tokens into a `CommandLineTool`
//! 2. `probe` optionally runs it once to discover which files it produces
//! 3. `requirements` attaches container, network, environment and mount requirements
//! 4. `save` post-processes, rebases paths on the target location, and serializes
//!
//! See [`crate::authoring`] for the same pipeline drawn out.

pub(crate) mod parser;
mod postprocess;
mod probe;
mod requirements;
mod save;

pub use requirements::ContainerInfo;

use crate::{
    authoring::{AuthoringError, AuthoringResult, paths},
    repository::{self, Repository},
};
use bon::Builder;
use commonwl::{documents::CommandLineTool, engine::ContainerEngine};
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Builder)]
pub struct ToolCreationOptions {
    /// The command line to convert, already split into tokens.
    #[builder(into)]
    pub command: Vec<String>,
    /// Output files to declare instead of discovering them by running the tool.
    #[builder(default, into)]
    pub outputs: Vec<String>,
    /// Extra inputs to declare on top of those parsed from `command`.
    #[builder(default, into)]
    pub inputs: Vec<String>,
    /// Name for the tool. Defaults to the script or command the tool runs.
    #[builder(into)]
    pub name: Option<String>,
    /// Where to write the tool, relative to the project root. Defaults to a per-tool folder
    /// under [`paths::WORKFLOWS_FOLDER`].
    #[builder(into)]
    pub output_dir: Option<PathBuf>,
    /// Write the tool to disk. When false, [`create_tool`] only returns it.
    #[builder(default)]
    pub save: bool,
    /// Skip the trial run that discovers outputs.
    #[builder(default)]
    pub no_run: bool,
    /// Delete whatever the trial run produced once its outputs are recorded.
    #[builder(default)]
    pub cleanup: bool,
    /// Stage and commit the tool and anything the trial run produced.
    #[builder(default)]
    pub commit: bool,
    /// Strip the default values parsed off the command line.
    #[builder(default)]
    pub clear_defaults: bool,
    pub container: Option<ContainerInfo>,
    #[builder(default)]
    pub enable_network: bool,
    #[builder(default, into)]
    pub mounts: Vec<PathBuf>,
    /// A `KEY=value` file whose entries the tool needs in its environment.
    #[builder(into)]
    pub env: Option<PathBuf>,
    /// Container engine to run the trial run under. `None` runs it directly.
    pub run_container: Option<ContainerEngine>,
}

/// A tool built by [`create_tool`].
#[derive(Debug, Clone)]
pub struct CreatedTool {
    /// Where the tool belongs, relative to the project root it was created in.
    pub path: PathBuf,
    /// The tool itself, post-processed exactly as it was serialized.
    pub document: CommandLineTool,
    /// `document` as formatted CWL YAML.
    pub yaml: String,
}

/// Builds a `CommandLineTool` from `options.command`, run against `project_root`.
///
/// `project_root` is the git repository the tool belongs to: paths are resolved against it,
/// the trial run executes there, and it is what gets checked for uncommitted changes.
pub async fn create_tool(
    project_root: &Path,
    options: &ToolCreationOptions,
) -> AuthoringResult<CreatedTool> {
    let mut cwl = create_tool_base(project_root, options).await?;

    if options.run_container.is_none() {
        requirements::add_tool_requirements(&mut cwl, options)?;
    } else if let Some(container) = &options.container
        && requirements::is_sif_image(&container.image)
    {
        //if run_container is some requirements are already set in create_tool_base()
        //just the docker requirements needs to be altered in case of sif file
        requirements::rewrite_sif_container_mut(&mut cwl, container);
    }

    // Finalize CWL
    let base_command = cwl.base_command.as_ref().unwrap();
    let name = options.name.as_deref();
    let output_dir = options.output_dir.clone().unwrap_or_else(|| {
        // default as in 1.x
        Path::new(paths::WORKFLOWS_FOLDER).join(paths::derive_tool_name(base_command, name))
    });
    let path = paths::get_qualified_filename(base_command, name, output_dir);
    let yaml = save::finalize_tool(&mut cwl, &path)?;

    if options.save {
        let repo =
            Repository::open(project_root).map_err(|source| AuthoringError::NoRepository {
                path: project_root.to_path_buf(),
                source,
            })?;
        save::save_tool_to_disk(&yaml, project_root, &path, &repo, options.commit)?;
    }

    Ok(CreatedTool {
        path,
        document: cwl,
        yaml,
    })
}

/// Parses the command line into a tool and, unless `options.no_run` is set, runs it once to
/// discover its outputs.
async fn create_tool_base(
    project_root: &Path,
    options: &ToolCreationOptions,
) -> AuthoringResult<CommandLineTool> {
    let command = options
        .command
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();

    //check for modified files and fail if there are any
    let repo = Repository::open(project_root).map_err(|source| AuthoringError::NoRepository {
        path: project_root.to_path_buf(),
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
        cwl.outputs = parser::outputs::get_outputs(&options.outputs);
    }

    if !options.inputs.is_empty() {
        parser::inputs::add_fixed_inputs(
            &mut cwl,
            &options
                .inputs
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
        )?;
    }

    if !options.no_run {
        let files =
            probe::run_and_collect_files(&mut cwl, project_root, options, &repo, &modified).await?;
        if options.outputs.is_empty() {
            cwl.outputs = parser::outputs::get_outputs(&files);
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

// Integration coverage of `create_tool` (this module's entry point) lives in
// `crates/sciwin/tests/tool_integration_test.rs` -- it only touches public API and drives
// real git repos and subprocesses end to end, which is integration-test territory.
