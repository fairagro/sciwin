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

#[cfg(test)]
mod tests {
    use super::*;
    use commonwl::{
        OneOrMany,
        documents::Argument,
        requirements::{
            DockerRequirement, Include, InitialWorkDirRequirement, NetworkAccess, StringOrInclude,
            WorkDirItems,
        },
        types::CWLType,
    };
    use fstest::fstest;
    use std::{env, fs};
    use test_utils::os_path;

    fn echo_options() -> ToolCreationOptions {
        ToolCreationOptions::builder()
            .command(vec!["echo".to_string(), "hello".to_string()])
            .build()
    }

    /// A dirty working tree is a state a frontend acts on (offer to commit, or to re-run
    /// with `no_run`), so it must arrive as a matchable variant, not a formatted string.
    #[fstest(repo = true, tokio = true, files = ["../../testdata/input.txt"])]
    pub async fn test_dirty_working_tree_is_a_typed_variant() {
        fs::write("uncommitted.txt", "scratch").unwrap();

        let root = env::current_dir().unwrap();
        let err = create_tool(&root, &echo_options()).await.unwrap_err();

        let AuthoringError::DirtyWorkingTree { files } = &err else {
            panic!("expected DirtyWorkingTree, got {err:?}");
        };
        assert!(files.iter().any(|f| f == "uncommitted.txt"));
    }

    /// Same reasoning: a missing repository is a state a frontend offers to fix.
    #[fstest(tokio = true)]
    pub async fn test_missing_repository_is_a_typed_variant() {
        let root = env::current_dir().unwrap();
        let err = create_tool(&root, &echo_options()).await.unwrap_err();

        assert!(
            matches!(err, AuthoringError::NoRepository { .. }),
            "expected NoRepository, got {err:?}"
        );
    }

    /// The tool is written where `path` says, relative to the project root -- not relative
    /// to whatever the process happens to have as its working directory.
    #[fstest(repo = true, tokio = true, files = ["../../testdata/input.txt"])]
    pub async fn test_saves_relative_to_project_root() {
        let root = env::current_dir().unwrap();
        let nested = root.join("nested");
        fs::create_dir(&nested).unwrap();
        // move the process somewhere else entirely; the project root is the argument
        env::set_current_dir(&nested).unwrap();

        let options = ToolCreationOptions::builder()
            .command(vec!["echo".to_string(), "hello".to_string()])
            .no_run(true)
            .save(true)
            .build();
        let created = create_tool(&root, &options).await.unwrap();

        assert_eq!(created.path, Path::new("workflows/echo/echo.cwl"));
        assert!(root.join(&created.path).is_file());
        assert!(!nested.join(&created.path).exists());
        //the document is handed back alongside the YAML, not just the string
        assert_eq!(
            created.document.base_command,
            Some(commonwl::OneOrMany::One("echo".to_string()))
        );
        assert_eq!(
            created.yaml,
            fs::read_to_string(root.join(&created.path)).unwrap()
        );

        env::set_current_dir(&root).unwrap();
    }

    // The tests below exercise `create_tool` (this module's entry point) directly, rather
    // than routing through the CLI's `CreateArgs`/`Commands` layer as before in old tool_integration_test.rs

    /// Recursively copies `src` into `dst`, creating `dst` if needed. Test-only stand-in for
    /// a `copy_dir` dependency, which crates/sciwin does not otherwise need.
    fn copy_dir(src: impl AsRef<Path>, dst: impl AsRef<Path>) {
        let (src, dst) = (src.as_ref(), dst.as_ref());
        fs::create_dir_all(dst).unwrap();
        for entry in fs::read_dir(src).unwrap() {
            let entry = entry.unwrap();
            let target = dst.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                copy_dir(entry.path(), target);
            } else {
                fs::copy(entry.path(), target).unwrap();
            }
        }
    }

    #[fstest(repo = true, tokio = true, files = ["../../testdata/input.txt", "../../testdata/echo.py"])]
    pub async fn tool_create_test() {
        let root = env::current_dir().unwrap();
        let options = ToolCreationOptions::builder()
            .command(vec![
                "python3".to_string(),
                "echo.py".to_string(),
                "--test".to_string(),
                "input.txt".to_string(),
            ])
            .save(true)
            .commit(true)
            .build();
        create_tool(&root, &options).await.unwrap();

        assert!(Path::new("results.txt").exists());
        assert!(Path::new("workflows/echo/echo.cwl").exists());

        //no uncommitted left?
        let repo = Repository::open(&root).unwrap();
        assert!(repository::get_modified_files(&repo).unwrap().is_empty());
    }

    #[fstest(repo = true, tokio = true, files = ["../../testdata/input.txt", "../../testdata/echo_inline.py"])]
    pub async fn tool_create_test_inputs_outputs() {
        fs::create_dir_all("data").unwrap();
        fs::copy("input.txt", "data/input.txt").unwrap(); //copy to data folder
        fs::remove_file("input.txt").unwrap(); //remove original file

        let root = env::current_dir().unwrap();
        let repo = Repository::open(&root).unwrap();
        repository::stage_all(&repo).unwrap();

        let options = ToolCreationOptions::builder()
            .command(vec!["python3".to_string(), "echo_inline.py".to_string()])
            .inputs(vec!["data/input.txt".to_string()])
            .outputs(vec!["results.txt".to_string()])
            .save(true)
            .commit(true)
            .build();
        let created = create_tool(&root, &options).await.unwrap();

        assert_eq!(
            created.path,
            Path::new("workflows/echo_inline/echo_inline.cwl")
        );
        assert!(Path::new("results.txt").exists());
        assert!(root.join(&created.path).is_file());

        assert_eq!(created.document.inputs.len(), 1);
        assert_eq!(created.document.outputs.len(), 1);

        let Some(iwdr) = created.document.get_requirement::<InitialWorkDirRequirement>() else {
            panic!("Tool does not contain an InitialWorkDirRequirement");
        };
        let WorkDirItems::ListingItems(listing) = &iwdr.listing else {
            panic!("InitialWorkDirRequirement does not contain listing items");
        };
        assert_eq!(listing.len(), 2);

        //no uncommitted left?
        assert!(repository::get_modified_files(&repo).unwrap().is_empty());
    }

    #[fstest(repo = true, tokio = true, files = ["../../testdata/input.txt", "../../testdata/echo.py"])]
    pub async fn tool_create_test_no_save() {
        let root = env::current_dir().unwrap();
        let options = ToolCreationOptions::builder()
            .command(vec![
                "python3".to_string(),
                "echo.py".to_string(),
                "--test".to_string(),
                "input.txt".to_string(),
            ])
            .commit(true)
            .build();
        create_tool(&root, &options).await.unwrap();

        assert!(!Path::new("workflows/echo/echo.cwl").exists()); //save was not requested
        assert!(Path::new("results.txt").exists());

        //no uncommitted left?
        let repo = Repository::open(&root).unwrap();
        assert!(repository::get_modified_files(&repo).unwrap().is_empty());
    }

    #[fstest(repo = true, tokio = true, files = ["../../testdata/input.txt", "../../testdata/echo.py"])]
    pub async fn tool_create_test_no_commit() {
        let root = env::current_dir().unwrap();
        let options = ToolCreationOptions::builder()
            .command(vec![
                "python3".to_string(),
                "echo.py".to_string(),
                "--test".to_string(),
                "input.txt".to_string(),
            ])
            .save(true) //look! no .commit(true)
            .build();
        create_tool(&root, &options).await.unwrap();

        //check for files being present
        assert!(Path::new("results.txt").exists());
        assert!(Path::new("workflows/echo/echo.cwl").exists());

        //as we did not commit there must be files (exactly 2, the cwl file and the results.txt)
        let repo = Repository::open(&root).unwrap();
        assert_eq!(repository::get_modified_files(&repo).unwrap().len(), 2);
    }

    #[fstest(repo = true, tokio = true, files = ["../../testdata/input.txt", "../../testdata/echo.py"])]
    pub async fn tool_create_test_no_run() {
        let root = env::current_dir().unwrap();
        let options = ToolCreationOptions::builder()
            .command(vec![
                "python3".to_string(),
                "echo.py".to_string(),
                "--test".to_string(),
                "input.txt".to_string(),
            ])
            .no_run(true)
            .save(true)
            .commit(true)
            .build();
        create_tool(&root, &options).await.unwrap();

        assert!(Path::new("workflows/echo/echo.cwl").exists());

        //no uncommitted left?
        let repo = Repository::open(&root).unwrap();
        assert!(repository::get_modified_files(&repo).unwrap().is_empty());
    }

    #[fstest(repo = true, tokio = true, files = ["../../testdata/input.txt", "../../testdata/echo.py", "../../testdata/data.bin"])]
    pub async fn tool_create_test_no_run_explicit_inputs() {
        let root = env::current_dir().unwrap();
        let options = ToolCreationOptions::builder()
            .command(vec![
                "python3".to_string(),
                "echo.py".to_string(),
                "--test".to_string(),
                "input.txt".to_string(),
            ])
            .inputs(vec!["data.bin".to_string()])
            .no_run(true)
            .save(true)
            .commit(true)
            .build();
        let created = create_tool(&root, &options).await.unwrap();

        assert!(Path::new("workflows/echo/echo.cwl").exists());
        assert!(
            created
                .document
                .inputs
                .iter()
                .any(|i| i.id == Some("data_bin".to_string()))
        );

        //no uncommitted left?
        let repo = Repository::open(&root).unwrap();
        assert!(repository::get_modified_files(&repo).unwrap().is_empty());
    }

    #[fstest(repo = true, tokio = true, files = ["../../testdata/input.txt", "../../testdata/echo.py"])]
    pub async fn tool_create_test_no_run_explicit_inputs_string() {
        let root = env::current_dir().unwrap();
        let options = ToolCreationOptions::builder()
            .command(vec![
                "python3".to_string(),
                "echo.py".to_string(),
                "--test".to_string(),
                "input.txt".to_string(),
            ])
            .inputs(vec!["wurstbrot".to_string()])
            .no_run(true)
            .save(true)
            .commit(true)
            .build();
        let created = create_tool(&root, &options).await.unwrap();

        assert!(Path::new("workflows/echo/echo.cwl").exists());
        assert!(
            created
                .document
                .inputs
                .iter()
                .any(|i| i.id == Some("wurstbrot".to_string()))
        );

        //no uncommitted left?
        let repo = Repository::open(&root).unwrap();
        assert!(repository::get_modified_files(&repo).unwrap().is_empty());
    }

    #[fstest(repo = true, tokio = true, files = ["../../testdata/input.txt", "../../testdata/echo.py"])]
    pub async fn tool_create_test_is_clean() {
        let root = env::current_dir().unwrap();
        let options = ToolCreationOptions::builder()
            .command(vec![
                "python3".to_string(),
                "echo.py".to_string(),
                "--test".to_string(),
                "input.txt".to_string(),
            ])
            .cleanup(true)
            .save(true)
            .commit(true)
            .build();
        create_tool(&root, &options).await.unwrap();

        assert!(Path::new("workflows/echo/echo.cwl").exists());
        assert!(!Path::new("results.txt").exists()); //no result is left as it is cleaned

        //no uncommitted left?
        let repo = Repository::open(&root).unwrap();
        assert!(repository::get_modified_files(&repo).unwrap().is_empty());
    }

    #[fstest(repo = true, tokio = true, files = ["../../testdata/input.txt", "../../testdata/echo.py"])]
    pub async fn tool_create_test_container_image() {
        let root = env::current_dir().unwrap();
        let options = ToolCreationOptions::builder()
            .command(vec![
                "python3".to_string(),
                "echo.py".to_string(),
                "--test".to_string(),
                "input.txt".to_string(),
            ])
            .container(ContainerInfo::builder().image("python3").build())
            .save(true)
            .commit(true)
            .build();
        let created = create_tool(&root, &options).await.unwrap();

        assert_eq!(created.document.requirements.as_ref().unwrap().len(), 2);

        let Some(dr) = created.document.get_requirement::<DockerRequirement>() else {
            panic!("Tool does not contain a DockerRequirement");
        };
        assert_eq!(dr.docker_pull.as_deref(), Some("python3"));

        //no uncommitted left?
        let repo = Repository::open(&root).unwrap();
        assert!(repository::get_modified_files(&repo).unwrap().is_empty());
    }

    #[fstest(repo = true, tokio = true, files = ["../../testdata/Dockerfile", "../../testdata/input.txt", "../../testdata/echo.py"])]
    pub async fn tool_create_test_dockerfile() {
        let root = env::current_dir().unwrap();
        let options = ToolCreationOptions::builder()
            .command(vec![
                "python3".to_string(),
                "echo.py".to_string(),
                "--test".to_string(),
                "input.txt".to_string(),
            ])
            .container(
                ContainerInfo::builder()
                    .image("Dockerfile")
                    .tag("sciwin-client")
                    .build(),
            )
            .save(true)
            .commit(true)
            .build();
        let created = create_tool(&root, &options).await.unwrap();

        assert_eq!(created.document.requirements.as_ref().unwrap().len(), 2);

        let Some(dr) = created.document.get_requirement::<DockerRequirement>() else {
            panic!("Tool does not contain a DockerRequirement");
        };
        let (Some(docker_file), Some(docker_image_id)) = (&dr.docker_file, &dr.docker_image_id)
        else {
            panic!("DockerRequirement does not contain dockerFile and dockerImageId");
        };
        assert_eq!(
            *docker_file,
            StringOrInclude::Include(Include {
                include: os_path("../../Dockerfile")
            })
        ); // as file is in root and CWL in workflows/echo
        assert_eq!(docker_image_id, "sciwin-client");

        //no uncommitted left?
        let repo = Repository::open(&root).unwrap();
        assert!(repository::get_modified_files(&repo).unwrap().is_empty());
    }

    #[fstest(repo = true, tokio = true)]
    pub async fn test_tool_magic_outputs() {
        let root = env::current_dir().unwrap();
        let options = ToolCreationOptions::builder()
            .command(shlex::split("touch output.txt").unwrap())
            .cleanup(true)
            .build();
        let created = create_tool(&root, &options).await.unwrap();

        assert_eq!(
            created.document.outputs[0]
                .output_binding
                .as_ref()
                .unwrap()
                .glob
                .clone()
                .unwrap()
                .as_one(),
            "$(inputs.output_txt)"
        );
    }

    #[fstest(repo = true, tokio = true, files = ["../../testdata/input.txt"])]
    pub async fn test_tool_magic_stdout() {
        let root = env::current_dir().unwrap();
        let options = ToolCreationOptions::builder()
            .command(shlex::split("wc input.txt \\> input.txt").unwrap())
            .cleanup(true)
            .build();
        let created = create_tool(&root, &options).await.unwrap();

        assert_eq!(created.document.stdout.unwrap(), "$(inputs.input_txt.path)");
    }

    #[fstest(repo = true, tokio = true, files = ["../../testdata/input.txt"])]
    pub async fn test_tool_magic_arguments() {
        let root = env::current_dir().unwrap();
        let options = ToolCreationOptions::builder()
            .command(shlex::split("cat input.txt | grep -f input.txt").unwrap())
            .cleanup(true)
            .build();
        let created = create_tool(&root, &options).await.unwrap();

        let Argument::Binding(binding) = &created.document.arguments.unwrap()[3] else {
            panic!("expected a binding argument");
        };
        assert_eq!(
            binding.value_from,
            Some("$(inputs.input_txt.path)".to_string())
        );
    }

    #[fstest(repo = true, tokio = true, files = ["../../testdata/create_dir.py"])]
    pub async fn test_tool_output_is_dir() {
        let root = env::current_dir().unwrap();
        let options = ToolCreationOptions::builder()
            .command(vec!["python3".to_string(), "create_dir.py".to_string()])
            .build();
        let created = create_tool(&root, &options).await.unwrap();

        assert_eq!(created.document.inputs.len(), 0);
        assert_eq!(created.document.outputs.len(), 1); //only folder
        assert_eq!(
            created.document.outputs[0].id,
            Some("my_directory".to_string())
        );
        assert_eq!(created.document.outputs[0].r#type, CWLType::Directory.into());
    }

    #[fstest(repo = true, tokio = true, files = ["../../testdata/create_dir.py"])]
    pub async fn test_tool_output_complete_dir() {
        let root = env::current_dir().unwrap();
        let options = ToolCreationOptions::builder()
            .command(vec!["python3".to_string(), "create_dir.py".to_string()])
            .outputs(vec![".".to_string()])
            .build();
        let created = create_tool(&root, &options).await.unwrap();

        assert_eq!(created.document.inputs.len(), 0);
        assert_eq!(created.document.outputs.len(), 1); //only root folder
        let Some(binding) = &created.document.outputs[0].output_binding else {
            panic!("No Binding")
        };
        assert_eq!(
            binding.glob,
            Some(OneOrMany::One("$(runtime.outdir)".to_string()))
        );
    }

    #[fstest(repo= true, tokio = true, files=["../../testdata/script.sh"])]
    #[cfg(target_os = "linux")]
    pub async fn test_shell_script() {
        use commonwl::requirements::ListingItems;

        std::fs::set_permissions(
            "script.sh",
            <std::fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o755),
        )
        .unwrap();
        let root = env::current_dir().unwrap();
        let repo = Repository::open(&root).unwrap();
        repository::stage_all(&repo).unwrap();

        let options = ToolCreationOptions::builder()
            .command(vec!["./script.sh".to_string()])
            .build();
        let created = create_tool(&root, &options).await.unwrap();

        assert_eq!(created.document.inputs.len(), 0);
        assert_eq!(created.document.outputs.len(), 0);
        assert_eq!(created.document.requirements.as_ref().unwrap().len(), 1);

        let Some(iwdr) = created.document.get_requirement::<InitialWorkDirRequirement>() else {
            panic!("Tool does not contain an InitialWorkDirRequirement");
        };

        let WorkDirItems::ListingItems(listing) = &iwdr.listing else {
            panic!("InitialWorkDirRequirement does not contain listing items");
        };
        assert_eq!(listing.len(), 1);

        let ListingItems::Dirent(dirent) = listing.first().unwrap() else {
            panic!("ListingItems is not of type Dirent");
        };
        assert_eq!(dirent.entryname, Some("./script.sh".to_string()));
    }

    #[fstest(repo = true, tokio = true)]
    /// see Issue [#89](https://github.com/fairagro/sciwin/issues/89)
    pub async fn test_tool_uncommitted_no_run() {
        let root = env::current_dir().unwrap();
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        fs::copy(
            format!("{manifest_dir}/../../testdata/input.txt"),
            "input.txt",
        )
        .unwrap(); //repo is not in a clean state now!

        let options = ToolCreationOptions::builder()
            .command(vec!["echo".to_string(), "Hello World".to_string()])
            .no_run(true)
            .build();
        //should be ok to not commit changes, as tool does not run
        assert!(create_tool(&root, &options).await.is_ok());
    }

    #[fstest(repo = true, tokio = true, files = ["../../testdata/subfolders.py"])]
    /// see Issue [#88](https://github.com/fairagro/sciwin/issues/88)
    pub async fn test_tool_output_subfolders() {
        let root = env::current_dir().unwrap();
        let options = ToolCreationOptions::builder()
            .command(vec!["python3".to_string(), "subfolders.py".to_string()])
            .build();
        //should be ok to not commit changes, as tool does not run
        assert!(create_tool(&root, &options).await.is_ok());
    }

    #[fstest(repo = true, tokio = true)]
    #[cfg(target_os = "linux")]
    pub async fn tool_create_remote_file() {
        let root = env::current_dir().unwrap();
        let options = ToolCreationOptions::builder()
            .command(vec![
                "cat".to_string(),
                "https://raw.githubusercontent.com/fairagro/sciwin/refs/heads/main/README.md"
                    .to_string(),
                ">".to_string(),
                "README.md".to_string(),
            ])
            .build();
        let created = create_tool(&root, &options).await.unwrap();

        //check file
        assert!(Path::new("README.md").exists());

        //check input
        assert_eq!(created.document.inputs.len(), 1);
        assert_eq!(created.document.inputs[0].r#type, CWLType::File.into());
    }

    #[fstest(repo = true, tokio = true, files = ["../../testdata/input.txt", "../../testdata/echo.py"])]
    pub async fn tool_create_test_network() {
        let root = env::current_dir().unwrap();
        let options = ToolCreationOptions::builder()
            .command(vec![
                "python3".to_string(),
                "echo.py".to_string(),
                "--test".to_string(),
                "input.txt".to_string(),
            ])
            .container(ContainerInfo::builder().image("python3").build())
            .enable_network(true)
            .build();
        let created = create_tool(&root, &options).await.unwrap();

        assert!(
            created
                .document
                .get_requirement::<NetworkAccess>()
                .is_some()
        );
    }

    #[fstest(repo = true, tokio = true)]
    pub async fn tool_create_same_inout() {
        let root = env::current_dir().unwrap();
        let options = ToolCreationOptions::builder()
            .command(vec![
                "echo".to_string(),
                "message".to_string(),
                ">".to_string(),
                "message".to_string(),
            ])
            .build();
        let created = create_tool(&root, &options).await.unwrap();

        assert!(
            created
                .document
                .inputs
                .iter()
                .any(|i| i.id == Some("message".to_string()))
        );
        //is not allowed to also have same id!
        assert!(
            !created
                .document
                .outputs
                .iter()
                .any(|i| i.id == Some("message".to_string()))
        );

        //decided to just prefix the output with "o_"
        //inputs are used by name, so we do not change them
        assert!(
            created
                .document
                .outputs
                .iter()
                .any(|i| i.id == Some("o_message".to_string()))
        );
    }

    #[fstest(repo = true, tokio = true)]
    pub async fn tool_create_mount() {
        let root = env::current_dir().unwrap();

        //copy a dir we can mount to the working directory
        copy_dir(
            format!("{}/../../testdata/test_dir", env!("CARGO_MANIFEST_DIR")),
            "test_dir",
        );
        let repo = Repository::open(&root).unwrap();
        repository::stage_all(&repo).unwrap();
        repository::commit(&repo, "message").unwrap();

        let options = ToolCreationOptions::builder()
            .command(vec![
                "ls".to_string(),
                ".".to_string(),
                ">".to_string(),
                "folder-list.txt".to_string(),
            ])
            .mounts(vec![PathBuf::from("test_dir")])
            .build();
        let created = create_tool(&root, &options).await.unwrap();

        let Some(iwdr) = created.document.get_requirement::<InitialWorkDirRequirement>() else {
            panic!("Tool does not contain an InitialWorkDirRequirement");
        };

        let WorkDirItems::ListingItems(listing) = &iwdr.listing else {
            panic!("InitialWorkDirRequirement does not contain listing items");
        };
        assert_eq!(listing.len(), 1);
    }

    #[fstest(repo = true, tokio = true)]
    pub async fn tool_create_typehint() {
        let root = env::current_dir().unwrap();
        let options = ToolCreationOptions::builder()
            .command(vec!["ls".to_string(), "s:.".to_string()]) //. would normally be a directory type. we enforce string here
            .build();
        let created = create_tool(&root, &options).await.unwrap();

        assert_eq!(created.document.inputs[0].r#type, CWLType::String.into());
    }
}
