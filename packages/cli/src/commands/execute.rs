use clap::{Args, Subcommand};
use commonwl::{
    OneOrMany,
    documents::CWLDocument,
    engine::{
        ContainerEngine, EngineStatus, InputObject, LocalBackend, create_execution_request,
        create_execution_request_with_inputs, evaluate_exitcodes,
    },
    files::{Directory, File, FileOrDirectory},
    inputs::{DefaultValue, InputSchema, InputType},
    types::CWLType,
};
use cwl_engine_storage::{StorageBackend, StoragePath};
use remote_execution::{check_status, download_results, export_rocrate, logout};
use s4n_core::parser::guess_type;
use serde_yaml::{Number, Value};
use std::{
    collections::HashMap,
    error::Error,
    fs,
    path::{Path, PathBuf},
    process::exit,
    sync::Arc,
};
use tokio_util::sync::CancellationToken;

pub async fn handle_execute_commands(subcommand: &ExecuteCommands) -> anyhow::Result<()> {
    match subcommand {
        ExecuteCommands::Local(args) => execute_local(args).await,
        ExecuteCommands::Remote(remote_args) => match &remote_args.command {
            RemoteSubcommands::Start {
                file,
                input_file,
                rocrate,
                watch,
                logout,
            } => schedule_run(file, input_file, *rocrate, *watch, *logout)
                .map_err(|e| anyhow::anyhow!("{e}")),
            RemoteSubcommands::Status { workflow_name } => {
                check_status(workflow_name).map_err(|e| anyhow::anyhow!("{e}"))
            }
            RemoteSubcommands::Download {
                workflow_name,
                all,
                output_dir,
            } => download_results(workflow_name, *all, output_dir.as_ref())
                .map_err(|e| anyhow::anyhow!("{e}")),
            RemoteSubcommands::Rocrate {
                workflow_name,
                output_dir,
            } => export_rocrate(workflow_name, output_dir.as_ref(), None)
                .map_err(|e| anyhow::anyhow!("{e}")),
            RemoteSubcommands::Logout => logout().map_err(|e| anyhow::anyhow!("{e}")),
        },
        ExecuteCommands::MakeTemplate(args) => make_template(&args.cwl),
    }
}

#[derive(Debug, Subcommand)]
pub enum ExecuteCommands {
    #[command(about = "Runs CWL files locally", visible_alias = "l")]
    Local(LocalExecuteArgs),
    #[command(about = "Runs CWL files remotely using reana", visible_alias = "r")]
    Remote(RemoteExecuteArgs),
    #[command(about = "Creates job file template for execution (e.g. inputs.yaml)")]
    MakeTemplate(MakeTemplateArgs),
}

#[derive(Args, Debug)]
pub struct MakeTemplateArgs {
    #[arg(help = "CWL File to create input template for")]
    pub cwl: PathBuf,
}

#[derive(Args, Debug, Default)]
pub struct LocalExecuteArgs {
    #[arg(long = "outdir", help = "A path to output resulting files to")]
    pub out_dir: Option<PathBuf>,
    #[arg(long = "quiet", help = "Runner does not print to stdout")]
    pub is_quiet: bool,
    #[arg(long = "podman", help = "Use podman instead of docker")]
    pub podman: bool,
    #[arg(long = "singularity", help = "Use singularity instead of docker")]
    pub singularity: bool,
    #[arg(long = "apptainer", help = "Use apptainer instead of docker")]
    pub apptainer: bool,
    #[arg(help = "CWL File to execute")]
    pub file: PathBuf,
    #[arg(
        trailing_var_arg = true,
        help = "Other arguments provided to cwl file",
        allow_hyphen_values = true
    )]
    pub args: Vec<String>,
}

#[derive(Debug, Args)]
pub struct RemoteExecuteArgs {
    #[command(subcommand)]
    pub command: RemoteSubcommands,
}

#[derive(Debug, Subcommand)]
pub enum RemoteSubcommands {
    #[command(about = "Schedules Execution on REANA")]
    Start {
        #[arg(help = "CWL File to execute")]
        file: PathBuf,
        #[arg(help = "Input YAML file")]
        input_file: Option<PathBuf>,
        #[arg(long = "rocrate", help = "Create Provenance Run Crate")]
        rocrate: bool,
        #[arg(
            long = "logout",
            help = "Delete reana information from credential storage (a.k.a logout)"
        )]
        logout: bool,
        #[arg(
            long = "watch",
            help = "Wait for workflow execution to finish and download result"
        )]
        watch: bool,
    },
    #[command(about = "Get the status of Execution on REANA")]
    Status {
        #[arg(help = "Workflow name to check (if omitted, checks all)")]
        workflow_name: Option<String>,
    },
    #[command(about = "Downloads workflow outputs from REANA")]
    Download {
        #[arg(help = "Workflow name to download results for")]
        workflow_name: String,
        #[arg(short = 'a', long = "all", help = "Download all files of the workflow")]
        all: bool,
        #[arg(
            short = 'd',
            long = "output_dir",
            help = "Optional output directory to save downloaded files"
        )]
        output_dir: Option<String>,
    },
    #[command(about = "Downloads finished Workflow Run RO-Crate from REANA")]
    Rocrate {
        #[arg(help = "Workflow name to create a Provenance Run Crate for")]
        workflow_name: String,
        #[arg(
            short = 'd',
            long = "rocrate_dir",
            default_value = "rocrate",
            help = "Optional directory to save RO-Crate to, default rocrate"
        )]
        output_dir: Option<String>,
    },
    #[command(about = "Delete reana information from credential storage (a.k.a logout)")]
    Logout,
}

#[allow(clippy::disallowed_macros)]
pub async fn execute_local(args: &LocalExecuteArgs) -> Result<(), anyhow::Error> {
    if args.is_quiet {
        log::set_max_level(log::LevelFilter::Error);
    }

    let container_engine = if args.podman {
        ContainerEngine::Podman
    } else if args.singularity {
        ContainerEngine::Singularity
    } else if args.apptainer {
        ContainerEngine::Apptainer
    } else {
        ContainerEngine::Docker
    };
    let storage = Arc::new(StorageBackend::new());
    let local_data_store = StoragePath::from_local(Path::new("/tmp"));
    let backend = Arc::new(LocalBackend::new(
        container_engine,
        storage,
        local_data_store,
    ));

    let request = if args.args.is_empty() {
        create_execution_request_with_inputs(
            args.file.clone(),
            InputObject::default(),
            args.out_dir.as_deref(),
            None,
        )?
    } else if args.args.len() == 1 && fs::exists(args.args[0].clone())? {
        create_execution_request(
            args.file.clone().clone(),
            args.args[0].clone(),
            args.out_dir.as_deref(),
        )?
    } else {
        let raw = args
            .args
            .chunks_exact(2)
            .filter_map(|pair| {
                if let Some(key) = pair[0].strip_prefix("--") {
                    let raw_value = &pair[1];
                    let value = match guess_type(raw_value) {
                        CWLType::File => DefaultValue::FileOrDirectory(FileOrDirectory::File(
                            File::builder().location(raw_value.to_string()).build(),
                        )),
                        CWLType::Directory => {
                            DefaultValue::FileOrDirectory(FileOrDirectory::Directory(
                                Directory::builder().location(raw_value.to_string()).build(),
                            ))
                        }
                        _ => DefaultValue::Any(
                            serde_yaml::from_str(raw_value).expect("Could not read input"),
                        ),
                    };
                    Some((key.to_string(), value))
                } else {
                    None
                }
            })
            .collect::<HashMap<_, _>>();
        let inputs = InputObject {
            inputs: raw,
            ..Default::default()
        };
        create_execution_request_with_inputs(
            args.file.clone(),
            inputs,
            args.out_dir.as_deref(),
            None,
        )?
    };
    let cancellation_token = CancellationToken::new();
    let result = commonwl::engine::execute(backend, &request, cancellation_token).await?;
    let exit_status = result.exit_status;

    let evaluated_code = evaluate_exitcodes(&exit_status, &request.specification);

    if let EngineStatus::Success(_) = evaluated_code {
        let json = serde_json::to_string_pretty(&result.outputs)?;
        println!("{json}");
        exit(0)
    } else {
        exit(1)
    }
}

pub fn schedule_run(
    file: &Path,
    input_file: &Option<PathBuf>,
    rocrate: bool,
    watch: bool,
    logout: bool,
) -> Result<(), Box<dyn Error>> {
    let workflow_name = remote_execution::schedule_run(file, input_file)?;

    if watch {
        remote_execution::watch(&workflow_name, rocrate)?;
    }

    if logout && let Err(e) = remote_execution::logout() {
        eprintln!("Error logging out of reana instance: {e}");
    }

    Ok(())
}

#[allow(clippy::disallowed_macros)]
pub fn make_template(filename: &PathBuf) -> anyhow::Result<()> {
    let template = make_template_impl(filename)?;
    let yaml = serde_yaml::to_string(&template)?;

    println!("{yaml}");
    Ok(())
}

fn make_template_impl(filename: &PathBuf) -> anyhow::Result<HashMap<String, DefaultValue>> {
    let contents = fs::read_to_string(filename)?;
    let cwl: CWLDocument = serde_yaml::from_str(&contents)?;

    Ok(cwl
        .get_inputs() //we assume there is no stdin
        .iter()
        .map(|i| {
            let id = &i.id;
            let dummy_value = if i.default.is_some() {
                return (id.clone().unwrap(), i.default.clone().unwrap());
            } else {
                get_default(&i.r#type)
            };
            (id.clone().unwrap(), dummy_value)
        })
        .collect::<HashMap<_, _>>())
}

fn get_default(r#type: &OneOrMany<InputType>) -> DefaultValue {
    let input_type = match r#type {
        OneOrMany::One(t) => t,
        OneOrMany::Many(ts) => ts.first().unwrap_or(&InputType::CWLType(CWLType::Null)),
    };

    match input_type {
        InputType::CWLType(cwltype) => fs_defaults(cwltype),
        InputType::InputSchema(schema) => match schema.as_ref() {
            InputSchema::Record(_) => DefaultValue::Any(Value::Mapping(Default::default())),
            InputSchema::Enum(e) => DefaultValue::Any(Value::String(
                e.symbols.first().cloned().unwrap_or_default(),
            )),
            InputSchema::Array(a) => DefaultValue::Any(Value::Sequence(vec![
                serde_yaml::to_value(get_default(&a.items)).unwrap_or(Value::Null),
                serde_yaml::to_value(get_default(&a.items)).unwrap_or(Value::Null),
            ])),
        },
        InputType::String(_) => DefaultValue::Any(Value::Null),
    }
}

fn fs_defaults(cwltype: &CWLType) -> DefaultValue {
    match cwltype {
        CWLType::File => DefaultValue::FileOrDirectory(FileOrDirectory::File(
            File::builder().location("./path/to/file.txt").build(),
        )),
        CWLType::Directory => DefaultValue::FileOrDirectory(FileOrDirectory::Directory(
            Directory::builder().location("./path/to/dir").build(),
        )),
        _ => DefaultValue::Any(cwltype_defaults(cwltype)),
    }
}

fn cwltype_defaults(cwltype: &CWLType) -> Value {
    match cwltype {
        CWLType::Boolean => Value::Bool(true),
        CWLType::Int | CWLType::Long => Value::Number(Number::from(42)),
        CWLType::Float | CWLType::Double => Value::Number(Number::from(69.42)),
        CWLType::String => Value::String("Hello World".into()),
        CWLType::Any => Value::String("Any Value".into()),
        _ => Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_defaults_type() {
        assert_eq!(
            cwltype_defaults(&CWLType::Int),
            Value::Number(Number::from(42))
        );
        assert_eq!(cwltype_defaults(&CWLType::Boolean), Value::Bool(true));
        assert_eq!(
            cwltype_defaults(&CWLType::Long),
            Value::Number(Number::from(42))
        );
        assert_eq!(
            cwltype_defaults(&CWLType::Float),
            Value::Number(Number::from(69.42))
        );
        assert_eq!(
            cwltype_defaults(&CWLType::String),
            Value::String("Hello World".into())
        );
        assert_eq!(
            cwltype_defaults(&CWLType::Any),
            Value::String("Any Value".into())
        );
    }

    #[test]
    fn test_default_values() {
        assert_eq!(
            fs_defaults(&CWLType::File),
            DefaultValue::FileOrDirectory(FileOrDirectory::File(
                File::builder().location("./path/to/file.txt").build()
            ))
        );
        assert_eq!(
            fs_defaults(&CWLType::Directory),
            DefaultValue::FileOrDirectory(FileOrDirectory::Directory(
                Directory::builder().location("./path/to/dir").build()
            ))
        );
        assert_eq!(
            fs_defaults(&CWLType::String),
            DefaultValue::Any(Value::String("Hello World".into()))
        );
    }

    #[test]
    fn test_make_template_impl() {
        let path = PathBuf::from("../../testdata/hello_world/workflows/main/main.cwl");
        let template = make_template_impl(&path).unwrap();
        let expected = HashMap::from([
            (
                "population".to_string(),
                DefaultValue::FileOrDirectory(FileOrDirectory::File(
                    File::builder()
                        .location("../../data/population.csv")
                        .build(),
                )),
            ),
            (
                "speakers".to_string(),
                DefaultValue::FileOrDirectory(FileOrDirectory::File(
                    File::builder()
                        .location("../../data/speakers_revised.csv")
                        .build(),
                )),
            ),
        ]);

        assert_eq!(template, expected);
    }
}
