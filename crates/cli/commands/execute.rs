use chrono::Utc;
use clap::{Args, Subcommand};
use sciwin::{
    cwl::{
        OneOrMany,
        documents::CWLDocument,
        engine::{ContainerEngine, InputObject, LocalBackend, load_input_file_from_file},
        files::{Directory, File, FileOrDirectory},
        inputs::{DefaultValue, InputSchema, InputType},
        storage::{StorageBackend, StoragePath},
        types::CWLType,
    },
    authoring::tool::parser::guess_type,
    execution::{ReanaRunner, RunStatus, TaskRunner, WorkflowRunner},
    project,
    provenance::{Written, reana_runner as rocrate_export, task_runner as local_rocrate_export},
    reana::{api::client::ReanaClient, auth::ReanaAccessToken, client as reana_client},
    rocrate::validate::Validation,
};
use serde_json::{Number, Value};
use std::{
    collections::HashMap,
    env, fs,
    path::{Path, PathBuf},
    sync::Arc,
};
use tracing::{info, warn};
use url::Url;

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
            } => execute_remote_start(file, input_file, *rocrate, *watch, *logout).await,
            RemoteSubcommands::Status { workflow_name } => {
                execute_remote_status(workflow_name.as_deref()).await
            }
            RemoteSubcommands::Download {
                workflow_name,
                all,
                output_dir,
            } => execute_remote_download(workflow_name, *all, output_dir.as_deref()).await,
            RemoteSubcommands::Rocrate {
                workflow_name,
                output_dir,
            } => execute_remote_rocrate(workflow_name, output_dir.as_deref()).await,
            RemoteSubcommands::Logout => {
                // crates/cli has no credential storage yet -- `reana_runner()` below reads
                // REANA_URL/REANA_TOKEN from the environment as a stopgap until a real
                // keyring-backed TokenProvider exists, so there's nothing to log out of.
                anyhow::bail!(
                    "Logout is not implemented yet: credentials are currently read from REANA_URL/REANA_TOKEN env vars, nothing is stored"
                )
            }
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
    #[arg(long = "podman", help = "Use podman instead of docker")]
    pub podman: bool,
    #[arg(long = "singularity", help = "Use singularity instead of docker")]
    pub singularity: bool,
    #[arg(long = "apptainer", help = "Use apptainer instead of docker")]
    pub apptainer: bool,
    #[arg(help = "CWL File to execute")]
    pub file: PathBuf,
    #[arg(
        long = "rocrate",
        num_args = 0..=1,
        default_missing_value = "files",
        value_enum,
        help = "Create a Provenance Run Crate after execution. Bare --rocrate (or --rocrate files) \
                keeps the original CWL files, directly re-executable; --rocrate packed writes one \
                packed JSON file instead"
    )]
    pub rocrate: Option<RocrateLayout>,
    #[arg(
        long = "rocrate_dir",
        default_value = "./rocrate",
        help = "Directory to save the RO-Crate to"
    )]
    pub rocrate_dir: PathBuf,
    #[arg(
        trailing_var_arg = true,
        help = "Other arguments provided to cwl file",
        allow_hyphen_values = true
    )]
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum RocrateLayout {
    /// One packed JSON file holds the whole workflow graph.
    Packed,
    /// The original CWL files, still directly re-executable. What bare `--rocrate` means.
    Files,
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
            default_value = "./rocrate",
            help = "Optional directory to save RO-Crate to, default ./rocrate"
        )]
        output_dir: Option<String>,
    },
    #[command(about = "Delete reana information from credential storage (a.k.a logout)")]
    Logout,
}

#[allow(clippy::disallowed_macros)]
pub async fn execute_local(args: &LocalExecuteArgs) -> Result<(), anyhow::Error> {
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
    let backend = Arc::new(LocalBackend::new(
        container_engine,
        storage,
        StoragePath::from_local(&env::temp_dir()),
    ));
    let runner = TaskRunner::new(backend);

    let base_path = dunce::canonicalize(args.file.parent().unwrap_or(Path::new(".")))?;
    let inputs = if args.args.is_empty() {
        InputObject::default()
    } else if args.args.len() == 1 && fs::exists(args.args[0].clone())? {
        load_input_file_from_file(args.args[0].clone(), base_path)?
    } else {
        let raw = args
            .args
            .chunks_exact(2)
            .filter_map(|pair| {
                if let Some(key) = pair[0].strip_prefix("--") {
                    let raw_value = &pair[1];
                    let value = match guess_type(raw_value, Path::new(".")) {
                        CWLType::File => DefaultValue::FileOrDirectory(FileOrDirectory::File(
                            File::builder().location(raw_value.to_string()).build(),
                        )),
                        CWLType::Directory => {
                            DefaultValue::FileOrDirectory(FileOrDirectory::Directory(
                                Directory::builder().location(raw_value.to_string()).build(),
                            ))
                        }
                        _ => DefaultValue::Any(
                            serde_saphyr::from_str(raw_value).expect("Could not read input"),
                        ),
                    };
                    Some((key.to_string(), value))
                } else {
                    None
                }
            })
            .collect::<HashMap<_, _>>();
        InputObject {
            inputs: raw,
            ..Default::default()
        }
    };

    // Not `run_workflow` (submit + stream logs + wait + print outputs, all in one): `--rocrate`
    // needs the run id afterward, which that convenience wrapper doesn't hand back. TaskRunner's
    // `logs()` is `NotSupported` anyway (subprocess output already goes to the terminal via
    // tracing as it runs), so nothing is lost by not using it here.
    let run_id = runner
        .submit(&args.file, inputs, args.out_dir.as_deref())
        .await?;
    let status = runner.wait_for_completion(&run_id).await?;

    #[allow(clippy::disallowed_macros)]
    match status {
        RunStatus::Finished => {
            if let Some(outputs) = runner.outputs(&run_id, args.out_dir.as_deref()).await? {
                println!("{}", serde_json::to_string_pretty(&outputs)?);
            }
        }
        _ => match runner.failure_detail(&run_id).await? {
            Some(detail) => anyhow::bail!("workflow ended with status {status:?}: {detail}"),
            None => anyhow::bail!("workflow ended with status {status:?}"),
        },
    }

    if let Some(layout_arg) = args.rocrate {
        let layout = match layout_arg {
            RocrateLayout::Packed => local_rocrate_export::CrateLayout::Packed,
            RocrateLayout::Files => local_rocrate_export::CrateLayout::Files,
        };
        let cwd = env::current_dir()?;
        let metadata = project::read_config(&cwd)?.workflow;

        let (written, validation) = local_rocrate_export::export(
            &runner,
            &run_id,
            metadata,
            &args.rocrate_dir,
            Utc::now(),
            layout,
        )
        .await?;

        report_rocrate_export(&written, &validation, "this run doesn't have on disk");
    }

    Ok(())
}

/// Builds a `ReanaRunner` from `REANA_URL`/`REANA_TOKEN` env vars.
fn reana_runner() -> anyhow::Result<ReanaRunner> {
    let url = env::var("REANA_URL").map_err(|_| {
        anyhow::anyhow!(
            "REANA_URL is not set (no credential storage is wired up yet, see reana_runner() in execute.rs)"
        )
    })?;
    let token = env::var("REANA_TOKEN").map_err(|_| {
        anyhow::anyhow!(
            "REANA_TOKEN is not set (no credential storage is wired up yet, see reana_runner() in execute.rs)"
        )
    })?;

    let server_url = Url::parse(&url)?;
    let token: Arc<ReanaAccessToken> = Arc::new(ReanaAccessToken::new(token));
    let client = ReanaClient::new(server_url.join("api")?, token);
    Ok(ReanaRunner::new(client))
}

#[allow(clippy::disallowed_macros)]
async fn execute_remote_start(
    file: &Path,
    input_file: &Option<PathBuf>,
    rocrate: bool,
    watch: bool,
    logout: bool,
) -> anyhow::Result<()> {
    let runner = reana_runner()?;

    let inputs = match input_file {
        Some(input_file) => {
            let base_path = dunce::canonicalize(file.parent().unwrap_or(Path::new(".")))?;
            load_input_file_from_file(input_file.clone(), base_path)?
        }
        None => InputObject::default(),
    };

    // `execution::reana_compat::compatibility_adjustments` isn't wired in here, yet
    let run_id = runner.submit(file, inputs, None).await?;
    info!("submitted workflow run '{run_id}'");

    if watch {
        let status = runner.wait_for_completion(&run_id).await?;
        info!("workflow '{run_id}' finished with status {status:?}");

        match status {
            RunStatus::Finished => {
                if let Some(outputs) = runner.outputs(&run_id, None).await? {
                    println!("{}", serde_json::to_string_pretty(&outputs)?);
                }
            }
            RunStatus::Failed => {
                runner.find_failures(&run_id).await?;
            }
            _ => {}
        }

        if rocrate {
            if status == RunStatus::Finished {
                export_rocrate(&run_id, runner.get_client(), Path::new("./rocrate")).await?;
            } else {
                warn!(
                    "--rocrate requested, but the run did not finish (status {status:?}); no crate was created"
                );
            }
        }
    }

    if logout {
        // See the `Logout` subcommand: no credential storage wired up yet.
        warn!("--logout requested, but nothing is stored to log out of yet");
    }

    Ok(())
}

async fn execute_remote_status(workflow_name: Option<&str>) -> anyhow::Result<()> {
    let runner = reana_runner()?;

    if let Some(name) = workflow_name {
        let status = runner.status(&name.to_string()).await?;
        info!("{name}: {status:?}");
        if matches!(status, RunStatus::Failed) {
            runner.find_failures(&name.to_string()).await?;
        }
        return Ok(());
    }

    let list = reana_client::list(runner.get_client()).await?;
    if list.items.is_empty() {
        info!("no workflows found for this REANA instance");
    }
    for workflow in list.items {
        info!(
            "{} ({}): {:?}",
            workflow.name,
            workflow.created,
            workflow.status.unwrap_or_default()
        );
    }

    Ok(())
}

async fn execute_remote_rocrate(
    workflow_name: &str,
    output_dir: Option<&str>,
) -> anyhow::Result<()> {
    let runner = reana_runner()?;
    let out_dir = PathBuf::from(output_dir.unwrap_or("./rocrate"));
    export_rocrate(workflow_name, runner.get_client(), &out_dir).await
}

/// Reads the project's `workflow.toml` from the current directory, exports the RO-Crate for
/// `workflow_name` into `output_dir`, and reports what came out of it 
async fn export_rocrate(
    workflow_name: &str,
    client: Arc<ReanaClient>,
    output_dir: &Path,
) -> anyhow::Result<()> {
    let cwd = env::current_dir()?;
    let metadata = project::read_config(&cwd)?.workflow;

    let (written, validation) =
        rocrate_export::export(client, workflow_name, metadata, output_dir, Utc::now()).await?;

    report_rocrate_export(&written, &validation, "REANA did not have on its workspace");

    Ok(())
}

/// Logs what an export produced: where the metadata landed, which referenced files the backend
/// didn't have (`missing_source` names whose fault that is -- REANA's workspace, or this run's
/// own disk), and any profile violations. The library builds and writes the crate regardless of
/// whether it's conformant; surfacing that is this CLI's job, not `sciwin::provenance`'s.
fn report_rocrate_export(written: &Written, validation: &Validation, missing_source: &str) {
    info!("RO-Crate written to {}", written.metadata.display());

    if !written.missing.is_empty() {
        warn!(
            "crate references files {missing_source}: {:?}",
            written.missing
        );
    }

    if !validation.is_conformant() {
        warn!("crate does not conform to its claimed profiles:");
        for error in validation.errors() {
            warn!("  {error}");
        }
    }
    for warning in validation.warnings() {
        warn!("{warning}");
    }
}

async fn execute_remote_download(
    workflow_name: &str,
    all: bool,
    output_dir: Option<&str>,
) -> anyhow::Result<()> {
    let runner = reana_runner()?;
    let out_dir = output_dir
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));

    if all {
        let client = runner.get_client();
        let workspace = reana_client::workspace(client.clone(), workflow_name).await?;
        for item in workspace.items {
            reana_client::download_file(client.clone(), workflow_name, &item.name, &out_dir)
                .await?;
        }
    } else {
        runner
            .outputs(&workflow_name.to_string(), Some(&out_dir))
            .await?;
    }

    Ok(())
}

#[allow(clippy::disallowed_macros)]
pub fn make_template(filename: &PathBuf) -> anyhow::Result<()> {
    let template = make_template_impl(filename)?;
    let yaml = serde_saphyr::to_string(&template)?;

    println!("{yaml}");
    Ok(())
}

fn make_template_impl(filename: &PathBuf) -> anyhow::Result<HashMap<String, DefaultValue>> {
    let contents = fs::read_to_string(filename)?;
    let cwl: CWLDocument = serde_saphyr::from_str(&contents)?;

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
            InputSchema::Record(_) => DefaultValue::Any(Value::Object(Default::default())),
            InputSchema::Enum(e) => DefaultValue::Any(Value::String(
                e.symbols.first().cloned().unwrap_or_default(),
            )),
            InputSchema::Array(a) => DefaultValue::Any(Value::Array(vec![
                serde_json::to_value(get_default(&a.items)).unwrap_or(Value::Null),
                serde_json::to_value(get_default(&a.items)).unwrap_or(Value::Null),
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
        CWLType::Float | CWLType::Double => Value::Number(Number::from_f64(69.42).unwrap()),
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
            Value::Number(Number::from_f64(69.42).unwrap())
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
