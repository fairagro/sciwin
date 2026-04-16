use crate::{
    io::{self, resolve_path},
    parser,
};
use anyhow::{Result, anyhow};
use commonwl::{
    Entry, EnviromentDefs, PathItem,
    execution::{ContainerEngine, environment::RuntimeEnvironment, io::create_and_write_file, runner::command, set_container_engine},
    format::format_cwl,
    prelude::*,
    requirements::WorkDirItem,
};
use repository::{self, Repository};
use std::{
    collections::HashMap,
    env,
    fs::{self, remove_file},
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
};

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

pub fn create_tool(options: &ToolCreationOptions, name: Option<String>, save: bool) -> Result<String> {
    let mut cwl = create_tool_base(options)?;

    if options.run_container.is_none() {
        cwl = add_tool_requirements(cwl, options.container.as_ref(), options.enable_network, options.mounts, options.env)?;
    } else if let Some(container) = &options.container
        && Path::new(container.image).extension().is_some_and(|ext| ext.eq_ignore_ascii_case("sif"))
    {
        //if run_container is some requirements are already set in create_tool_base()
        //just the docker requirements needs to be altered in case of sif file
        cwl.base.requirements.retain(|req| !matches!(req, Requirement::DockerRequirement(_)));

        let mut modified_container = container.clone();
        modified_container.image = container.image.trim_end_matches(".sif");
        cwl = append_container_requirement(cwl, Some(&modified_container));
    }
    // Finalize CWL
    let path = io::get_qualified_filename(&cwl.base_command, name);
    let yaml = finalize_tool(&mut cwl, &path)?;

    if save {
        let cwd = env::current_dir()?;
        let repo = Repository::open(&cwd).map_err(|e| anyhow::anyhow!("Could not find git repository at {cwd:?}: {e}"))?;
        save_tool_to_disk(&yaml, &path, &repo, options.commit)?;
    }
    Ok(yaml)
}

fn save_tool_to_disk(yaml: &str, path: &String, repo: &Repository, commit: bool) -> Result<()> {
    match create_and_write_file(path, yaml) {
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

fn create_tool_base(options: &ToolCreationOptions) -> Result<CommandLineTool> {
    let command: &[String] = options.command;
    let outputs: &[String] = options.outputs;
    let inputs: &[String] = options.inputs;
    let no_run: bool = options.no_run;
    let cleanup: bool = options.cleanup;
    let commit: bool = options.commit;
    let clear_defaults: bool = options.clear_defaults;
    let env: Option<&Path> = options.env;
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
        cwl = cwl.with_outputs(parser::get_outputs(outputs));
    }

    if !no_run {
        let mut environment = RuntimeEnvironment::default();
        if let Some(env_file) = env {
            environment = environment.with_environment(read_env(env_file)?);
        }

        if let Some(engine) = &run_container {
            match engine {
                ContainerEngine::Singularity | ContainerEngine::Apptainer => {
                    set_container_engine(ContainerEngine::Singularity);
                }
                _ => {
                    set_container_engine(*engine);
                }
            }
            cwl = add_tool_requirements(cwl, options.container.as_ref(), options.enable_network, options.mounts, options.env)?;
            command::run_command(&cwl, &mut environment).map_err(|e| anyhow::anyhow!("Failed to run tool: {e}"))?;
        } else {
            command::run_command(&cwl, &mut environment)
                .map_err(|e| anyhow::anyhow!("Could not execute command: `{}`: {}!", command.join(" "), e))?;
        }

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
            cwl = cwl.with_outputs(parser::get_outputs(&files));
        }
    }

    if !inputs.is_empty() {
        parser::add_fixed_inputs(&mut cwl, &inputs.iter().map(String::as_str).collect::<Vec<_>>())
            .map_err(|e| anyhow!("Could not gather fixed inputs: {e}"))?;
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
        cwl = cwl.append_requirement(Requirement::NetworkAccess(NetworkAccess { network_access: true }));
    }

    if let Some(env) = env {
        let data = read_env(env)?;
        cwl = cwl.append_requirement(Requirement::EnvVarRequirement(EnvVarRequirement {
            env_def: EnviromentDefs::Map(data),
        }));
    }

    if !mounts.is_empty() {
        let entries = mounts.iter().filter_map(|m| {
            if m.is_dir() {
                Some(WorkDirItem::FileOrDirectory(Box::new(DefaultValue::Directory(Directory::from_path(m)))))
            } else {
                eprintln!("{} is not a directory and has been skipped!", m.display());
                None
            }
        });

        if let Some(iwdr) = cwl.get_requirement_mut::<InitialWorkDirRequirement>() {
            iwdr.listing.extend(entries);
        } else {
            let iwdr = InitialWorkDirRequirement { listing: entries.collect() };
            if !iwdr.listing.is_empty() {
                cwl = cwl.append_requirement(Requirement::InitialWorkDirRequirement(iwdr));
            }
        }
    }
    Ok(cwl)
}

fn append_container_requirement(cwl: CommandLineTool, container: Option<&ContainerInfo>) -> CommandLineTool {
    if let Some(container) = &container {
        let requirement = if container.image.contains("Dockerfile") {
            let image_id = container.tag.unwrap_or("sciwin-container");
            Requirement::DockerRequirement(DockerRequirement::from_file(container.image, image_id))
        } else {
            Requirement::DockerRequirement(DockerRequirement::from_pull(container.image))
        };

        cwl.append_requirement(requirement)
    } else {
        cwl
    }
}

fn finalize_tool(cwl: &mut CommandLineTool, path: &str) -> Result<String> {
    parser::post_process_cwl(cwl);
    let mut yaml = prepare_save(cwl, path);
    yaml = format_cwl(&yaml).map_err(|e| anyhow::anyhow!("Failed to format CWL: {e}"))?;
    Ok(yaml)
}

fn prepare_save(tool: &mut CommandLineTool, path: &str) -> String {
    //rewire paths to new location
    for input in &mut tool.inputs {
        if let Some(DefaultValue::File(value)) = &mut input.default {
            value.location = Some(resolve_path(value.get_location(), path));
        }
        if let Some(DefaultValue::Directory(value)) = &mut input.default {
            value.location = Some(resolve_path(value.get_location(), path));
        }
    }

    for requirement in &mut tool.requirements {
        if let Requirement::DockerRequirement(docker) = requirement {
            if let Some(Entry::Include(include)) = &mut docker.docker_file {
                include.include = resolve_path(&include.include, path);
            }
        } else if let Requirement::InitialWorkDirRequirement(iwdr) = requirement {
            for listing in &mut iwdr.listing {
                if let WorkDirItem::Dirent(dirent) = listing
                    && let Entry::Include(include) = &mut dirent.entry
                {
                    include.include = resolve_path(&include.include, path);
                }
            }
        }
    }
    tool.to_string()
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_yaml::Value;
    use test_utils::os_path;

    #[test]
    pub fn test_cwl_save() {
        let inputs = vec![
            CommandInputParameter::default()
                .with_id("positional1")
                .with_default_value(DefaultValue::File(File::from_location("testdata/input.txt")))
                .with_type(CWLType::String)
                .with_binding(CommandLineBinding::default().with_position(0)),
            CommandInputParameter::default()
                .with_id("option1")
                .with_type(CWLType::String)
                .with_binding(CommandLineBinding::default().with_prefix("--option1"))
                .with_default_value(DefaultValue::Any(Value::String("value1".to_string()))),
        ];
        let mut clt = CommandLineTool::default()
            .with_base_command(Command::Multiple(vec!["python".to_string(), "test/script.py".to_string()]))
            .with_inputs(inputs)
            .with_requirements(vec![
                Requirement::InitialWorkDirRequirement(InitialWorkDirRequirement::from_file("test/script.py")),
                Requirement::DockerRequirement(DockerRequirement::from_file("test/data/Dockerfile", "test")),
            ]);

        prepare_save(&mut clt, "workflows/tool/tool.cwl");

        //check if paths are rewritten upon tool saving

        assert_eq!(
            clt.inputs[0].default,
            Some(DefaultValue::File(File::from_location(&os_path("../../testdata/input.txt"))))
        );
        let requirements = &clt.requirements;
        let req_0 = &requirements[0];
        let req_1 = &requirements[1];
        assert_eq!(
            *req_0,
            Requirement::InitialWorkDirRequirement(InitialWorkDirRequirement {
                listing: vec![WorkDirItem::Dirent(Dirent {
                    entry: Entry::from_file(&os_path("../../test/script.py")),
                    entryname: Some("test/script.py".to_string()),
                    ..Default::default()
                })]
            })
        );
        assert_eq!(
            *req_1,
            Requirement::DockerRequirement(DockerRequirement {
                docker_file: Some(Entry::from_file(&os_path("../../test/data/Dockerfile"))),
                docker_image_id: Some("test".to_string()),
                ..Default::default()
            })
        );
    }
}
