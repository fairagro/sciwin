use chrono::Utc;
use clap::{Args, Subcommand};
use miette::IntoDiagnostic;
use sciwin::{
    cwl::{
        OneOrMany,
        documents::CWLDocument,
        engine::{ContainerEngine, InputObject, TaskBackend, load_input_file_from_file},
        files::{Directory, File, FileOrDirectory},
        inputs::{DefaultValue, InputSchema, InputType},
        types::CWLType,
    },
    execution::{
        ReanaRunner, RunId, RunStatus, TaskRunner, WorkflowRunner, docker_backend, local_backend,
        rocrate as rocrate_input, tes_backend,
    },
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
use tracing::{debug, info, warn};
use url::Url;

pub async fn handle_execute_commands(subcommand: &ExecuteCommands) -> miette::Result<()> {
    match subcommand {
        ExecuteCommands::Run(args) => execute_run(args).await,
        ExecuteCommands::Reana(reana_args) => match &reana_args.command {
            ReanaSubcommands::Status { workflow_name } => {
                execute_reana_status(workflow_name.as_deref()).await
            }
            ReanaSubcommands::Download {
                workflow_name,
                all,
                output_dir,
            } => execute_reana_download(workflow_name, *all, output_dir.as_deref()).await,
            ReanaSubcommands::Rocrate {
                workflow_name,
                output_dir,
            } => execute_reana_rocrate(workflow_name, output_dir.as_deref()).await,
        },
        ExecuteCommands::MakeTemplate(args) => make_template(&args.cwl),
    }
}

#[derive(Debug, Subcommand)]
pub enum ExecuteCommands {
    #[command(
        about = "Runs a CWL file with the selected execution engine",
        alias = "r",
        hide = true
    )]
    Run(RunArgs),
    #[command(about = "Queries or fetches results of runs already submitted to REANA")]
    Reana(ReanaArgs),
    #[command(about = "Creates job file template for execution (e.g. inputs.yaml)")]
    MakeTemplate(MakeTemplateArgs),
}

#[derive(Args, Debug)]
pub struct MakeTemplateArgs {
    #[arg(help = "CWL File to create input template for")]
    pub cwl: PathBuf,
}

#[derive(Args, Debug)]
pub struct RunArgs {
    #[arg(
        long = "engine",
        value_enum,
        default_value_t = ExecutionEngine::Local,
        help = "local (commonwl's runner, only containerizes DockerRequirement steps -- see \
                --runtime), docker (every step containerized via Docker directly), tes (submit \
                to a GA4GH TES server, needs TES_URL/TES_STORAGE env vars), reana (submit to \
                REANA, needs REANA_URL/REANA_TOKEN -- waits for completion unless --detach is \
                given)"
    )]
    pub engine: ExecutionEngine,
    #[arg(
        long = "runtime",
        value_enum,
        default_value_t = ContainerRuntime::Docker,
        help = "Container runtime for DockerRequirement steps. Only meaningful for --engine \
                local; --engine docker always uses Docker directly"
    )]
    pub runtime: ContainerRuntime,
    #[arg(long = "outdir", help = "A path to output resulting files to")]
    pub out_dir: Option<PathBuf>,
    #[arg(
        help = "CWL file to execute, or a Workflow/Run RO-Crate to execute (a directory or .zip \
                archive holding a ro-crate-metadata.json whose mainEntity is CWL)"
    )]
    pub file: PathBuf,
    #[arg(help = "Input YAML/JSON job file")]
    pub input_file: Option<PathBuf>,
    #[arg(
        long = "rocrate",
        num_args = 0..=1,
        default_missing_value = "files",
        value_enum,
        help = "Create a Provenance Run Crate after execution. Bare --rocrate (or --rocrate files) \
                keeps the original CWL files, directly re-executable; --rocrate packed writes one \
                packed JSON file instead. Cannot be combined with --detach. For --engine reana \
                the export is always packed regardless of the layout requested here (REANA only \
                ever returns one packed specification)"
    )]
    pub rocrate: Option<RocrateLayout>,
    #[arg(
        long = "rocrate_dir",
        default_value = "./rocrate",
        help = "Directory to save the RO-Crate to"
    )]
    pub rocrate_dir: PathBuf,
    #[arg(
        long = "detach",
        help = "Submit and return immediately without waiting for the run to finish. Only valid \
                with --engine reana (local/docker/tes keep no state to reconnect to after this \
                process exits); check progress later with `s4n execute reana status`. Cannot be \
                combined with --rocrate"
    )]
    pub detach: bool,
}

/// Mirrors `commonwl::engine::ContainerEngine` as a local, clap-`ValueEnum`-able type -- the
/// orphan rule blocks deriving `ValueEnum` on the foreign type directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum ContainerRuntime {
    #[default]
    Docker,
    Podman,
    Singularity,
    Apptainer,
}

impl From<ContainerRuntime> for ContainerEngine {
    fn from(value: ContainerRuntime) -> Self {
        match value {
            ContainerRuntime::Docker => ContainerEngine::Docker,
            ContainerRuntime::Podman => ContainerEngine::Podman,
            ContainerRuntime::Singularity => ContainerEngine::Singularity,
            ContainerRuntime::Apptainer => ContainerEngine::Apptainer,
        }
    }
}

/// Named `ExecutionEngine`, not `Engine`, to avoid confusion with the unrelated
/// `sciwin::provenance::inputs::Engine` (provenance metadata, not a dispatch selector).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum ExecutionEngine {
    #[default]
    Local,
    Docker,
    Tes,
    Reana,
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum RocrateLayout {
    /// One packed JSON file holds the whole workflow graph.
    Packed,
    /// The original CWL files, still directly re-executable. What bare `--rocrate` means.
    Files,
}

#[derive(Args, Debug)]
pub struct ReanaArgs {
    #[command(subcommand)]
    pub command: ReanaSubcommands,
}

#[derive(Debug, Subcommand)]
pub enum ReanaSubcommands {
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
}

pub async fn execute_run(args: &RunArgs) -> miette::Result<()> {
    if args.detach && args.engine != ExecutionEngine::Reana {
        miette::bail!(
            "--detach is only supported with --engine reana (local/docker/tes have no persisted run state to reconnect to)"
        );
    }
    if args.detach && args.rocrate.is_some() {
        miette::bail!(
            "--detach cannot be combined with --rocrate (crate export needs the finished run's outputs)"
        );
    }

    let resolved = rocrate_input::looks_like_crate(&args.file)
        .then(|| rocrate_input::resolve_target(&args.file))
        .transpose()?;
    let (cwl_path, base_path) = match &resolved {
        Some(resolved) => (resolved.cwl_path.clone(), resolved.base_dir.clone()),
        None => (
            args.file.clone(),
            dunce::canonicalize(args.file.parent().unwrap_or(Path::new(".")))
                .into_diagnostic()?,
        ),
    };
    if resolved.is_some() {
        info!(
            "resolved RO-Crate {} to workflow {}",
            args.file.display(),
            cwl_path.display()
        );
    }

    let inputs = match &args.input_file {
        Some(input_file) => load_input_file_from_file(input_file.clone(), base_path)?,
        None => InputObject::default(),
    };

    if args.engine == ExecutionEngine::Reana {
        execute_run_reana(args, &cwl_path, inputs).await
    } else {
        execute_run_task_backend(args, &cwl_path, inputs).await
    }
}

/// `--engine local|docker|tes`: always blocks (submit + wait) -- `TaskRunner` keeps in-process
/// state only, there is no other way for one CLI invocation to report a result.
async fn execute_run_task_backend(
    args: &RunArgs,
    cwl_path: &Path,
    inputs: InputObject,
) -> miette::Result<()> {
    let backend: Arc<dyn TaskBackend> = match args.engine {
        ExecutionEngine::Local => local_backend(args.runtime.into()),
        ExecutionEngine::Docker => docker_backend().await?,
        ExecutionEngine::Tes => tes_backend().await?,
        ExecutionEngine::Reana => unreachable!("handled by execute_run_reana"),
    };
    let runner = TaskRunner::new(backend);

    debug!("submitting {:?} run for {}", args.engine, cwl_path.display());
    let run_id = runner
        .submit(cwl_path, inputs, args.out_dir.as_deref())
        .await?;
    debug!("run '{run_id}' submitted, waiting for completion");
    wait_and_report(&runner, &run_id, args.out_dir.as_deref()).await?;

    if let Some(layout_arg) = args.rocrate {
        let layout = match layout_arg {
            RocrateLayout::Packed => local_rocrate_export::CrateLayout::Packed,
            RocrateLayout::Files => local_rocrate_export::CrateLayout::Files,
        };
        let cwd = env::current_dir().into_diagnostic()?;
        let metadata = project::read_config(&cwd)?.workflow;
        debug!(
            "exporting Provenance Run Crate ({layout:?}) to {:?}",
            args.rocrate_dir
        );

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

/// `--engine reana`: waits for completion by default, same as every other engine; `--detach`
/// submits and returns immediately instead -- the only engine that can, since REANA persists run
/// state server-side, so a later `s4n execute reana status`/`download`/`rocrate` can still reach
/// it after this process exits.
async fn execute_run_reana(
    args: &RunArgs,
    cwl_path: &Path,
    inputs: InputObject,
) -> miette::Result<()> {
    let runner = reana_runner()?;

    debug!("submitting remote run for {}", cwl_path.display());
    // `execution::reana_compat::compatibility_adjustments` isn't wired in here, yet
    let run_id = runner.submit(cwl_path, inputs, None).await?;
    info!("submitted workflow run '{run_id}'");

    if args.detach {
        info!(
            "--detach given; not waiting. Check progress with `s4n execute reana status {run_id}`"
        );
        return Ok(());
    }

    wait_and_report(&runner, &run_id, None).await?;

    if let Some(layout_arg) = args.rocrate {
        if matches!(layout_arg, RocrateLayout::Files) {
            warn!(
                "--rocrate=files was requested, but a REANA export is always packed (REANA only \
                 ever hands back one packed specification); exporting packed instead"
            );
        }
        export_rocrate(&run_id, runner.get_client(), &args.rocrate_dir).await?;
    }

    Ok(())
}

/// Waits for `id` to reach a terminal status, prints its outputs as pretty JSON on success, bails
/// with `failure_detail()` otherwise. Shared by every engine, since all of them wait by default
/// now.
///
/// Behavior note: a failed REANA run used to just log and return `Ok(())` here (only when
/// `--watch` was given); this now bails like local/docker/tes always have, for every engine.
#[allow(clippy::disallowed_macros)]
async fn wait_and_report(
    runner: &(dyn WorkflowRunner + Sync),
    run_id: &RunId,
    out_dir: Option<&Path>,
) -> miette::Result<()> {
    let status = runner.wait_for_completion(run_id).await?;
    match status {
        RunStatus::Finished => {
            if let Some(outputs) = runner.outputs(run_id, out_dir).await? {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&outputs).into_diagnostic()?
                );
            }
            Ok(())
        }
        _ => match runner.failure_detail(run_id).await? {
            Some(detail) => miette::bail!("workflow ended with status {status:?}: {detail}"),
            None => miette::bail!("workflow ended with status {status:?}"),
        },
    }
}

/// Builds a `ReanaRunner` from `REANA_URL`/`REANA_TOKEN` env vars (settable directly, or via a
/// `.env` file -- see `dotenvy::dotenv()` in `main.rs`).
fn reana_runner() -> miette::Result<ReanaRunner> {
    let url = env::var("REANA_URL")
        .map_err(|_| miette::miette!("REANA_URL is not set (see `s4n execute run --help`)"))?;
    let token = env::var("REANA_TOKEN")
        .map_err(|_| miette::miette!("REANA_TOKEN is not set (see `s4n execute run --help`)"))?;

    let server_url = Url::parse(&url).into_diagnostic()?;
    debug!("connecting to REANA at {server_url}");
    let token: Arc<ReanaAccessToken> = Arc::new(ReanaAccessToken::new(token));
    let client = ReanaClient::new(server_url.join("api").into_diagnostic()?, token);
    Ok(ReanaRunner::new(client))
}

async fn execute_reana_status(workflow_name: Option<&str>) -> miette::Result<()> {
    let runner = reana_runner()?;

    if let Some(name) = workflow_name {
        debug!("querying status for workflow '{name}'");
        let status = runner.status(&name.to_string()).await?;
        info!("{name}: {status:?}");
        if matches!(status, RunStatus::Failed) {
            runner.find_failures(&name.to_string()).await?;
        }
        return Ok(());
    }

    debug!("listing all workflows known to this REANA instance");
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

async fn execute_reana_rocrate(
    workflow_name: &str,
    output_dir: Option<&str>,
) -> miette::Result<()> {
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
) -> miette::Result<()> {
    let cwd = env::current_dir().into_diagnostic()?;
    let metadata = project::read_config(&cwd)?.workflow;
    debug!("exporting Provenance Run Crate for '{workflow_name}' to {output_dir:?}");

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

async fn execute_reana_download(
    workflow_name: &str,
    all: bool,
    output_dir: Option<&str>,
) -> miette::Result<()> {
    let runner = reana_runner()?;
    let out_dir = output_dir
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));

    if all {
        debug!("--all given, downloading every file in the workspace to {out_dir:?}");
        let client = runner.get_client();
        let workspace = reana_client::workspace(client.clone(), workflow_name).await?;
        debug!("workspace has {} item(s)", workspace.items.len());
        for item in workspace.items {
            debug!("downloading {}", item.name);
            reana_client::download_file(client.clone(), workflow_name, &item.name, &out_dir)
                .await?;
        }
    } else {
        debug!("downloading only declared outputs to {out_dir:?}");
        runner
            .outputs(&workflow_name.to_string(), Some(&out_dir))
            .await?;
    }

    Ok(())
}

#[allow(clippy::disallowed_macros)]
pub fn make_template(filename: &PathBuf) -> miette::Result<()> {
    let template = make_template_impl(filename)?;
    let yaml = serde_saphyr::to_string(&template).into_diagnostic()?;

    println!("{yaml}");
    Ok(())
}

fn make_template_impl(filename: &PathBuf) -> miette::Result<HashMap<String, DefaultValue>> {
    debug!("reading CWL document at {filename:?} to derive its inputs");
    let contents = fs::read_to_string(filename).into_diagnostic()?;
    let cwl: CWLDocument = serde_saphyr::from_str(&contents).into_diagnostic()?;

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
