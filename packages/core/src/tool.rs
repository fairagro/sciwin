use crate::{
    append_requirement,
    io::{self, resolve_path},
    parser,
};
use anyhow::{Result, anyhow};
use commonwl::{
    BoolOrExpression, OneOrMany,
    documents::{CWLDocument, CommandLineTool},
    engine::{
        ContainerEngine, InputObject, LocalBackend, create_execution_request_from_document,
        execute_commandline_tool,
    },
    files::{Directory, FileOrDirectory},
    format::format_cwl,
    inputs::DefaultValue,
    outputs::{
        CommandOutputArraySchema, CommandOutputBinding, CommandOutputParameter,
        CommandOutputSchema, CommandOutputType,
    },
    requirements::{
        DockerRequirement, EnvVarRequirement, EnvironmentDef, Include, InitialWorkDirRequirement,
        ListingItems, NetworkAccess, StringOrInclude, ToolRequirements, WorkDirItems,
    },
    types::CWLType,
};
use cwl_engine_storage::{StorageBackend, StoragePath};
use repository::{self, Repository};
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
}

pub async fn create_tool(
    options: &ToolCreationOptions<'_>,
    name: Option<String>,
    save: bool,
) -> Result<String> {
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
        cwl = append_container_requirement(cwl, Some(&modified_container));
    }
    // Finalize CWL
    let path = io::get_qualified_filename(cwl.base_command.as_ref().unwrap(), name);
    let yaml = finalize_tool(&mut cwl, &path)?;

    if save {
        let cwd = env::current_dir()?;
        let repo = Repository::open(&cwd)
            .map_err(|e| anyhow::anyhow!("Could not find git repository at {cwd:?}: {e}"))?;
        save_tool_to_disk(&yaml, &path, &repo, options.commit)?;
    }
    Ok(yaml)
}

fn save_tool_to_disk(yaml: &str, path: &String, repo: &Repository, commit: bool) -> Result<()> {
    let parent = Path::new(path).parent().unwrap();
    fs::create_dir_all(parent)
        .map_err(|e| anyhow::anyhow!("Failed to create directories for {parent:?}: {e}"))?;
    match fs::write(path, yaml) {
        Ok(()) => {
            if commit {
                repository::stage_file(repo, path)?;
                repository::commit(repo, &format!("🪄 Creation of `{path}`"))?;
            }
        }
        Err(e) => anyhow::bail!("Creation of File {path} failed: {e}"),
    }
    Ok(())
}

async fn create_tool_base(options: &ToolCreationOptions<'_>) -> Result<CommandLineTool> {
    let command: &[String] = options.command;
    let outputs: &[String] = options.outputs;
    let inputs: &[String] = options.inputs;
    let no_run: bool = options.no_run;
    let cleanup: bool = options.cleanup;
    let commit: bool = options.commit;
    let clear_defaults: bool = options.clear_defaults;
    let run_container = options.run_container;

    let command = command.iter().map(String::as_str).collect::<Vec<_>>();
    let current_working_dir = env::current_dir()?;

    //check for modified files and fail if there are any
    let repo = Repository::open(&current_working_dir)?;
    let modified = repository::get_modified_files(&repo);

    if !no_run && !modified.is_empty() {
        anyhow::bail!("Uncommitted changes detected: {:?}", modified);
    }

    //parse command
    let mut cwl = parser::parse_command_line(&command);

    // handle outputs
    if !outputs.is_empty() {
        cwl.outputs = parser::get_outputs(outputs);
    }

    if !inputs.is_empty() {
        parser::add_fixed_inputs(
            &mut cwl,
            &inputs.iter().map(String::as_str).collect::<Vec<_>>(),
        )
        .map_err(|e| anyhow!("Could not gather fixed inputs: {e}"))?;
    }
    if !no_run {
        let storage = Arc::new(StorageBackend::new());
        let backend = Arc::new(LocalBackend::new(
            run_container.unwrap_or(ContainerEngine::Docker),
            storage,
            StoragePath::from_local(Path::new("/tmp")),
        ));

        if run_container.is_some() {
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

        let job = create_execution_request_from_document(
            CWLDocument::CommandLineTool(clone_cwl),
            InputObject::default(),
            env::current_dir().unwrap(),
            Some(&env::current_dir().unwrap()),
            None,
        )?;
        let cancellation_token = CancellationToken::new();
        execute_commandline_tool(backend, &job, cancellation_token).await?;

        // Handle modified files
        let mut files = repository::get_modified_files(&repo);
        files.retain(|f| !modified.contains(f));

        if cleanup {
            for file in &files {
                remove_file(file)?;
            }
        }

        if commit {
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

        if outputs.is_empty() {
            cwl.outputs = parser::get_outputs(&files);
        }
    }

    // Clear defaults if requested
    if clear_defaults {
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
) -> Result<CommandLineTool> {
    // Handle container requirements
    cwl = append_container_requirement(cwl, container);

    if enable_network {
        append_requirement(
            &mut cwl,
            ToolRequirements::NetworkAccess(NetworkAccess {
                network_access: BoolOrExpression::Bool(true),
            }),
        );
    }

    if let Some(env) = env {
        let data = read_env(env)?;
        append_requirement(
            &mut cwl,
            ToolRequirements::EnvVarRequirement(
                EnvVarRequirement::builder()
                    .env_def(
                        data.iter()
                            .map(|(k, v)| {
                                EnvironmentDef::builder().env_name(k).env_value(v).build()
                            })
                            .collect(),
                    )
                    .build(),
            ),
        );
    }

    if !mounts.is_empty() {
        let entries = mounts.iter().filter_map(|m| {
            if m.is_dir() {
                Some(FileOrDirectory::Directory(
                    Directory::builder()
                        .location(m.to_string_lossy().into_owned())
                        .build(),
                ))
            } else {
                eprintln!("{} is not a directory and has been skipped!", m.display());
                None
            }
        });

        if let Some(iwdr) = cwl.get_requirement_mut::<InitialWorkDirRequirement>() {
            match &mut iwdr.listing {
                WorkDirItems::Expression(s) => {
                    let mut items: Vec<ListingItems> =
                        entries.map(ListingItems::FileOrDirectory).collect();
                    items.push(ListingItems::Expression(s.clone()));
                    iwdr.listing =
                        WorkDirItems::ListingItems(Box::new(commonwl::OneOrMany::Many(items)));
                }
                WorkDirItems::ListingItems(one_or_many) => match &mut **one_or_many {
                    OneOrMany::One(item) => {
                        **one_or_many = OneOrMany::Many(
                            vec![item.clone()]
                                .into_iter()
                                .chain(entries.map(ListingItems::FileOrDirectory))
                                .collect(),
                        );
                    }
                    OneOrMany::Many(items) => {
                        items.extend(entries.map(ListingItems::FileOrDirectory));
                    }
                },
            }
        } else {
            let items: Vec<ListingItems> = entries.map(ListingItems::FileOrDirectory).collect();
            if !items.is_empty() {
                let iwdr = InitialWorkDirRequirement {
                    listing: WorkDirItems::ListingItems(Box::new(OneOrMany::Many(items))),
                };
                append_requirement(&mut cwl, ToolRequirements::InitialWorkDirRequirement(iwdr));
            }
        }
    }
    Ok(cwl)
}

fn append_container_requirement(
    mut cwl: CommandLineTool,
    container: Option<&ContainerInfo>,
) -> CommandLineTool {
    if let Some(container) = &container {
        let requirement = if container.image.contains("Dockerfile") {
            let image_id = container.tag.unwrap_or("sciwin-container");
            ToolRequirements::DockerRequirement(
                DockerRequirement::builder()
                    .docker_file(StringOrInclude::Include(
                        Include::builder()
                            .include(container.image.to_owned())
                            .build(),
                    ))
                    .docker_image_id(image_id)
                    .build(),
            )
        } else {
            ToolRequirements::DockerRequirement(
                DockerRequirement::builder()
                    .docker_pull(container.image)
                    .build(),
            )
        };

        append_requirement(&mut cwl, requirement);
    }
    cwl
}

fn finalize_tool(cwl: &mut CommandLineTool, path: &str) -> Result<String> {
    parser::post_process_cwl(cwl)?;
    let mut yaml = prepare_save(cwl, path)?;
    yaml = format_cwl(&yaml).map_err(|e| anyhow::anyhow!("Failed to format CWL: {e}"))?;
    Ok(yaml)
}

fn prepare_save(tool: &mut CommandLineTool, path: &str) -> Result<String, serde_yaml::Error> {
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
                match &mut **listing {
                    OneOrMany::One(item) => {
                        if let ListingItems::Dirent(dirent) = item
                            && let StringOrInclude::Include(include) = &mut dirent.entry
                        {
                            include.include = resolve_path(&include.include, path);
                        }
                    }
                    OneOrMany::Many(items) => {
                        for item in items {
                            if let ListingItems::Dirent(dirent) = item
                                && let StringOrInclude::Include(include) = &mut dirent.entry
                            {
                                include.include = resolve_path(&include.include, path);
                            }
                        }
                    }
                }
            }
        }
    }
    serde_yaml::to_string(&CWLDocument::CommandLineTool(tool.clone()))
}

fn read_env(path: &Path) -> Result<HashMap<String, String>> {
    let f = fs::File::open(path)?;
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
    let staged_js = format!(
        "[{}]",
        staged_roots
            .iter()
            .map(|s| format!("\"{}\"", s))
            .collect::<Vec<_>>()
            .join(",")
    );
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
            WorkDirItems::ListingItems(oom) => match &**oom {
                OneOrMany::One(item) => get_iwdr_roots_for_item(item, &mut roots),
                OneOrMany::Many(items) => {
                    for item in items {
                        get_iwdr_roots_for_item(item, &mut roots);
                    }
                }
            },
        }
    }
    roots
}

fn get_iwdr_roots_for_item(item: &ListingItems, roots: &mut Vec<String>) {
    if let ListingItems::Dirent(dirent) = item {
        if let Some(ename) = &dirent.entryname {
            roots.push(root_path(ename));
        }
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
    use serde_yaml::Value;
    use test_utils::os_path;

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
                "python".to_string(),
                "test/script.py".to_string(),
            ]))
            .inputs(inputs)
            .requirements(vec![
                ToolRequirements::InitialWorkDirRequirement(InitialWorkDirRequirement {
                    listing: WorkDirItems::ListingItems(Box::new(OneOrMany::One(
                        ListingItems::Dirent(
                            Dirent::builder()
                                .entry(StringOrInclude::Include(
                                    Include::builder()
                                        .include(os_path("test/script.py"))
                                        .build(),
                                ))
                                .entryname("test/script.py".to_string())
                                .build(),
                        ),
                    ))),
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

        prepare_save(&mut clt, "workflows/tool/tool.cwl").unwrap();

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
                listing: WorkDirItems::ListingItems(Box::new(OneOrMany::One(
                    ListingItems::Dirent(
                        Dirent::builder()
                            .entry(StringOrInclude::Include(
                                Include::builder()
                                    .include(os_path("../../test/script.py"))
                                    .build()
                            ))
                            .entryname("test/script.py".to_string())
                            .build()
                    )
                ))),
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
