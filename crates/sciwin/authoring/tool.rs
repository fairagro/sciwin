use crate::{
    authoring::{
        AuthoringError, AuthoringResult, parser,
        paths::{self, resolve_path},
    },
    repository::{self, Repository},
};
use anyhow::Context as _;
use commonwl::{
    OneOrMany,
    documents::{CWLDocument, CommandLineTool},
    engine::{
        ContainerEngine, InputObject, LocalBackend, create_execution_request_from_document, execute,
    },
    files::{Directory, FileOrDirectory},
    format::format_cwl,
    inputs::DefaultValue,
    outputs::{
        CommandOutputArraySchema, CommandOutputBinding, CommandOutputParameter,
        CommandOutputSchema, CommandOutputType,
    },
    requirements::{
        InitialWorkDirRequirement, InlineJavascriptRequirement, ListingItems, StringOrInclude,
        ToolRequirements, WorkDirItems,
    },
    storage::{StorageBackend, StoragePath},
    types::CWLType,
};
use std::{
    collections::HashMap,
    env,
    fs::{self, remove_file},
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub struct ContainerInfo<'a> {
    pub image: &'a str,
    pub tag: Option<&'a str>,
}

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

pub async fn create_tool(
    options: &ToolCreationOptions<'_>,
    name: Option<String>,
    save: bool,
) -> AuthoringResult<String> {
    let mut cwl = create_tool_base(options).await?;

    if options.run_container.is_none() {
        cwl = add_tool_requirements(
            cwl,
            options.container.as_ref(),
            options.enable_network,
            options.mounts,
            options.env,
        )?;
    } else if let Some(container) = &options.container
        && Path::new(container.image)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("sif"))
    {
        //if run_container is some requirements are already set in create_tool_base()
        //just the docker requirements needs to be altered in case of sif file
        if let Some(reqs) = &mut cwl.requirements {
            reqs.retain(|req| !matches!(req, ToolRequirements::DockerRequirement(_)));
        }

        let mut modified_container = container.clone();
        modified_container.image = container.image.trim_end_matches(".sif");
        append_container_requirement_mut(&mut cwl, Some(&modified_container));
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
    let yaml = finalize_tool(&mut cwl, &path)?;

    if save {
        let cwd = env::current_dir()?;
        let repo = Repository::open(&cwd)
            .map_err(|source| AuthoringError::NoRepository { path: cwd, source })?;
        save_tool_to_disk(&yaml, &path, &repo, options.commit)?;
    }
    Ok(yaml)
}

fn save_tool_to_disk(
    yaml: &str,
    path: &Path,
    repo: &Repository,
    commit: bool,
) -> AuthoringResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create directories for {}", parent.display()))?;
    }
    fs::write(path, yaml)
        .with_context(|| format!("Creation of file {} failed", path.display()))?;

    if commit {
        repository::stage_file(repo, path)?;
        repository::commit(repo, &format!("🪄 Creation of `{}`", path.display()))?;
    }
    Ok(())
}

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
            path: current_working_dir.clone(),
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
        let storage = Arc::new(StorageBackend::new());
        let backend = Arc::new(LocalBackend::new(
            options.run_container.unwrap_or(ContainerEngine::Docker),
            storage,
            StoragePath::from_local(&env::temp_dir()),
        ));

        if options.run_container.is_some() {
            cwl = add_tool_requirements(
                cwl,
                options.container.as_ref(),
                options.enable_network,
                options.mounts,
                options.env,
            )?;
        }

        let mut clone_cwl = cwl.clone();
        clone_cwl.outputs.push(
            CommandOutputParameter::builder()
                .id("catch_all")
                .output_binding(
                    CommandOutputBinding::builder()
                        .glob(commonwl::OneOrMany::One("*".to_string()))
                        .output_eval(filter_output(&cwl)) //if something strange happens: this is the culprit
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
                .build(),
        );
        clone_cwl.append_requirement_mut(ToolRequirements::InlineJavascriptRequirement(
            InlineJavascriptRequirement::default(),
        ));

        let job = create_execution_request_from_document(
            CWLDocument::CommandLineTool(clone_cwl),
            InputObject::default(),
            env::current_dir().unwrap(),
            None,
            None,
        )?;
        let cancellation_token = CancellationToken::new();
        execute(backend, &job, cancellation_token).await?;

        // Handle modified files
        let mut files = repository::get_modified_files(&repo)?;
        files.retain(|f| !modified.contains(f));

        if options.cleanup {
            for file in &files {
                remove_file(file).with_context(|| format!("Failed to remove {file}"))?;
            }
        }

        if options.commit {
            for file in &files {
                let path = Path::new(file);
                if path.exists() {
                    if path.is_dir() {
                        repository::stage_dir(&repo, path)?;
                    } else {
                        repository::stage_file(&repo, file)?;
                    }
                }
            }
        }

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

fn add_tool_requirements(
    mut cwl: CommandLineTool,
    container: Option<&ContainerInfo>,
    enable_network: bool,
    mounts: &[PathBuf],
    env: Option<&Path>,
) -> AuthoringResult<CommandLineTool> {
    // Handle container requirements
    append_container_requirement_mut(&mut cwl, container);

    if enable_network {
        cwl.use_network_mut();
    }

    if let Some(env) = env {
        let data = read_env(env)?;
        cwl.use_env_requirement_mut(&data);
    }

    if !mounts.is_empty() {
        let entries = mounts
            .iter()
            .filter_map(|m| {
                if m.is_dir() {
                    Some(FileOrDirectory::Directory(
                        Directory::builder()
                            .location(m.to_string_lossy().into_owned())
                            .build(),
                    ))
                } else {
                    tracing::warn!("{} is not a directory and has been skipped!", m.display());
                    None
                }
            })
            .collect();
        cwl.add_mount_requirement_mut(entries);
    }
    Ok(cwl)
}

fn append_container_requirement_mut(cwl: &mut CommandLineTool, container: Option<&ContainerInfo>) {
    if let Some(container) = &container {
        if container.image.contains("Dockerfile") {
            let image_id = container.tag.unwrap_or("sciwin-container");
            cwl.use_docker_file_mut(container.image, image_id);
        } else {
            cwl.use_docker_pull_mut(container.image);
        }
    }
}

fn finalize_tool(cwl: &mut CommandLineTool, path: &Path) -> AuthoringResult<String> {
    parser::post_process_cwl(cwl)?;
    let mut yaml = prepare_save(cwl, path)?;
    yaml = format_cwl(&yaml)
        .map_err(|e| AuthoringError::Unknown(anyhow::anyhow!("Failed to format CWL: {e}")))?;
    Ok(yaml)
}

fn prepare_save(tool: &mut CommandLineTool, path: &Path) -> AuthoringResult<String> {
    //rewire paths to new location
    for input in &mut tool.inputs {
        if let Some(DefaultValue::FileOrDirectory(FileOrDirectory::File(value))) =
            &mut input.default
        {
            value.location = Some(resolve_path(value.location.as_ref().unwrap(), path));
        }
        if let Some(DefaultValue::FileOrDirectory(FileOrDirectory::Directory(value))) =
            &mut input.default
        {
            value.location = Some(resolve_path(value.location.as_ref().unwrap(), path));
        }
    }

    if let Some(requirements) = &mut tool.requirements {
        for requirement in requirements {
            if let ToolRequirements::DockerRequirement(docker) = requirement {
                if let Some(StringOrInclude::Include(include)) = &mut docker.docker_file {
                    include.include = resolve_path(&include.include, path);
                }
            } else if let ToolRequirements::InitialWorkDirRequirement(iwdr) = requirement
                && let WorkDirItems::ListingItems(listing) = &mut iwdr.listing
            {
                for item in listing {
                    if let ListingItems::Dirent(dirent) = item
                        && let StringOrInclude::Include(include) = &mut dirent.entry
                    {
                        include.include = resolve_path(&include.include, path);
                    }
                }
            }
        }
    }
    Ok(serde_saphyr::to_string(&CWLDocument::CommandLineTool(
        tool.clone(),
    ))?)
}

fn read_env(path: &Path) -> AuthoringResult<HashMap<String, String>> {
    let f = fs::File::open(path).with_context(|| format!("Failed to open env file {path:?}"))?;
    let reader = BufReader::new(f);

    let mut map = HashMap::new();
    for line in reader.lines() {
        if let Some((key, value)) = line?.split_once('=') {
            map.insert(key.to_string(), value.to_string());
        }
    }
    Ok(map)
}

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

fn get_iwdr_roots(cwl: &CommandLineTool) -> Vec<String> {
    let mut roots = vec![];
    if let Some(iwdr) = cwl.get_requirement::<InitialWorkDirRequirement>() {
        match &iwdr.listing {
            WorkDirItems::Expression(_) => {}
            WorkDirItems::ListingItems(items) => {
                for item in items {
                    get_iwdr_roots_for_item(item, &mut roots);
                }
            }
        }
    }
    roots
}

fn get_iwdr_roots_for_item(item: &ListingItems, roots: &mut Vec<String>) {
    if let ListingItems::Dirent(dirent) = item
        && let Some(ename) = &dirent.entryname
    {
        roots.push(root_path(ename));
    }
}

fn root_path(p: impl AsRef<Path>) -> String {
    p.as_ref()
        .components()
        .next()
        .unwrap()
        .as_os_str()
        .to_string_lossy()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use commonwl::{
        OneOrMany,
        files::{Dirent, File, FileOrDirectory},
        inputs::{CommandInputParameter, CommandLineBinding, DefaultValue},
        requirements::{
            DockerRequirement, Include, InitialWorkDirRequirement, ListingItems, StringOrInclude,
            WorkDirItems,
        },
        types::CWLType,
    };
    use fstest::fstest;
    use serde_json::Value;
    use test_utils::os_path;

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

    #[test]
    pub fn test_cwl_save() {
        let inputs = vec![
            CommandInputParameter::builder()
                .id("positional1")
                .default(DefaultValue::FileOrDirectory(FileOrDirectory::File(
                    File::builder().location("testdata/input.txt").build(),
                )))
                .r#type(CWLType::String)
                .input_binding(CommandLineBinding::builder().position(0).build())
                .build(),
            CommandInputParameter::builder()
                .id("option1")
                .r#type(CWLType::String)
                .input_binding(CommandLineBinding::builder().prefix("--option1").build())
                .default(DefaultValue::Any(Value::String("value1".to_string())))
                .build(),
        ];
        let mut clt = CommandLineTool::builder()
            .base_command(OneOrMany::Many(vec![
                "python3".to_string(),
                "test/script.py".to_string(),
            ]))
            .inputs(inputs)
            .requirements(vec![
                ToolRequirements::InitialWorkDirRequirement(InitialWorkDirRequirement {
                    listing: WorkDirItems::ListingItems(vec![ListingItems::Dirent(
                        Dirent::builder()
                            .entry(StringOrInclude::Include(
                                Include::builder()
                                    .include(os_path("test/script.py"))
                                    .build(),
                            ))
                            .entryname("test/script.py".to_string())
                            .build(),
                    )]),
                }),
                ToolRequirements::DockerRequirement(
                    DockerRequirement::builder()
                        .docker_file(StringOrInclude::Include(
                            Include::builder()
                                .include(os_path("test/data/Dockerfile"))
                                .build(),
                        ))
                        .docker_image_id("test")
                        .build(),
                ),
            ])
            .build();

        prepare_save(&mut clt, Path::new("workflows/tool/tool.cwl")).unwrap();

        //check if paths are rewritten upon tool saving

        assert_eq!(
            clt.inputs[0].default,
            Some(DefaultValue::FileOrDirectory(FileOrDirectory::File(
                File::builder()
                    .location(os_path("../../testdata/input.txt"))
                    .build()
            )))
        );
        let requirements = clt.requirements.as_ref().unwrap();
        let req_0 = &requirements[0];
        let req_1 = &requirements[1];
        assert_eq!(
            *req_0,
            ToolRequirements::InitialWorkDirRequirement(InitialWorkDirRequirement {
                listing: WorkDirItems::ListingItems(vec![ListingItems::Dirent(
                    Dirent::builder()
                        .entry(StringOrInclude::Include(
                            Include::builder()
                                .include(os_path("../../test/script.py"))
                                .build()
                        ))
                        .entryname("test/script.py".to_string())
                        .build()
                )]),
            })
        );
        assert_eq!(
            *req_1,
            ToolRequirements::DockerRequirement(
                DockerRequirement::builder()
                    .docker_file(StringOrInclude::Include(
                        Include::builder()
                            .include(os_path("../../test/data/Dockerfile"))
                            .build()
                    ))
                    .docker_image_id("test".to_string())
                    .build()
            )
        );
    }
}
