use clap::{Args, Subcommand};
use commonwl::execution::error::ExecutionError;
use commonwl::execution::{ContainerEngine, execute_cwlfile, set_container_engine};
use commonwl::prelude::*;
use remote_execution::{check_status, download_results, logout};
use serde_yaml::{Number, Value};
use std::{collections::HashMap, error::Error, fs, path::{PathBuf, Path}, sync::Arc};
use anyhow::anyhow;
use rocrate_ext::{unzip_rocrate, RocrateRunType, RocrateArgs, find_cwl_and_yaml_in_rocrate, verify_cwl_references, clone_from_rocrate_or_cwl, export_rocrate};
use commonwl::{load_doc, packed::pack_workflow};
use remote_execution::{reana_login};
use reana::{api::{get_workflow_logs, get_workflow_specification}, reana::Reana, utils::get_cwl_name};
use anyhow::Context;

pub async fn handle_execute_commands(subcommand: &ExecuteCommands) -> Result<(), Box<dyn Error>> {
    match subcommand {
        ExecuteCommands::Local(args) => {
            let mut args = args.clone();
            args.finalize();
            execute_local(&args).await?;
            if let Some(rocrate_args) = args.rocrate.as_ref() {
                export_as_rocrate(
                    args.file.to_string_lossy().as_ref(),
                    rocrate_args.output_dir.clone(),
                    rocrate_args.run_type,
                    Some("local"),
                ).await?;
            }
        }
        ExecuteCommands::Remote(remote_args) => match &remote_args.command {
            RemoteSubcommands::Start {
                file,
                input_file,
                rocrate,
                watch,
                logout,
            } => {
                schedule_run(file, input_file, rocrate, *watch, *logout).await?;
                if let Some(rocrate_args) = rocrate.as_ref() {
                    export_as_rocrate(
                        "remote",
                        rocrate_args.output_dir.clone(),
                        rocrate_args.run_type,
                        Some("remote"),
                    ).await?;
                }
            }
            RemoteSubcommands::Status { workflow_name } => check_status(workflow_name).await?,
            RemoteSubcommands::Download { workflow_name, all, output_dir } => download_results(workflow_name, *all, output_dir.as_ref()).await?,
            RemoteSubcommands::Rocrate(args) => export_as_rocrate(&args.workflow_name.clone().unwrap(), args.output_dir.clone(), args.run_type, Some("remote")).await?,
            RemoteSubcommands::Logout => logout()?,
        },
        ExecuteCommands::MakeTemplate(args) => make_template(&args.cwl)?,
    }
    Ok(())
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

#[derive(Args, Debug, Default, Clone)]
pub struct LocalExecuteArgs {
    #[arg(long = "outdir", help = "A path to output resulting files to")]
    pub out_dir: Option<String>,
    #[arg(long = "quiet", help = "Runner does not print to stdout")]
    pub is_quiet: bool,
    #[arg(long = "podman", help = "Use podman instead of docker")]
    pub podman: bool,
    #[arg(long = "singularity", help = "Use singularity instead of docker")]
    pub singularity: bool,
    #[arg(long = "apptainer", help = "Use apptainer instead of docker")]
    pub apptainer: bool,
    #[command(flatten)]
    pub rocrate: Option<RocrateArgs>,
    #[arg(help = "CWL File to execute")]
    pub file: PathBuf,
    #[arg(trailing_var_arg = true, help = "Other arguments provided to cwl file", allow_hyphen_values = true)]
    pub args: Vec<String>,
}

impl LocalExecuteArgs {
    pub fn finalize(&mut self) {
        if let Some(ref mut rocrate) = self.rocrate {
            if rocrate.workflow_name.is_some() {
            } else {
                rocrate.workflow_name = Some(self.file.to_string_lossy().to_string());
            }
        }
    }
}

#[derive(Debug, Subcommand)]
pub enum RemoteSubcommands {
    #[command(about = "Schedules Execution on REANA")]
    Start {
        #[arg(help = "CWL File to execute")]
        file: PathBuf,
        #[arg(help = "Input YAML file")]
        input_file: Option<PathBuf>,
        #[command(flatten)]
        rocrate: Option<RocrateArgs>,
        #[arg(long = "logout", help = "Delete reana information from credential storage (a.k.a logout)")]
        logout: bool,
        #[arg(long = "watch", help = "Wait for workflow execution to finish and download result")]
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
        #[arg(short = 'd', long = "output_dir", help = "Optional output directory to save downloaded files")]
        output_dir: Option<String>,
    },
    #[command(about = "Downloads finished Workflow Run RO-Crate from REANA")]
    Rocrate(RocrateArgs),
    #[command(about = "Delete reana information from credential storage (a.k.a logout)")]
    Logout,
}


#[derive(Debug, Args)]
pub struct RemoteExecuteArgs {
    #[command(subcommand)]
    pub command: RemoteSubcommands,
}

pub async fn export_as_rocrate(workflow_name: &str, output_dir: Option<String>,
    run_type: RocrateRunType, execution: Option<&str>) -> Result<(), Box<dyn Error>> {
    if execution == Some("remote") {
        let (reana_instance, reana_token) = reana_login()?;
        let reana = Reana::new(reana_instance, reana_token);
        let r = Arc::new(reana);
        let graph = get_workflow_specification(&r, workflow_name).await?;
        let graph_array = graph
            .get("specification")
            .and_then(|spec| spec.get("workflow"))
            .and_then(|s| s.get("specification"))
            .and_then(|wf| wf.get("$graph"))
            .and_then(|g| g.as_array())
            .ok_or_else(|| "Expected graph array".to_string())?;
        let logs = get_workflow_logs(&r, workflow_name).await?;
        let cwl_file = get_cwl_name(Some(r), workflow_name).await?;
        let working_dir = std::env::current_dir()?;
        match export_rocrate(
            output_dir.as_ref(),
            Some(&working_dir.to_string_lossy().to_string()),
            cwl_file.as_ref(),
            run_type,
            Some("remote"),
            graph_array,
            Some(&logs),
        ).await {
            Ok(_) => {}
            Err(e) => eprintln!("Error trying to create a RO-Crate: {e}"),
        }
    } else {
        let doc = load_doc(workflow_name)?;
        let CWLDocument::Workflow(workflow) = doc else {
            return Err(Box::new(ExecutionError::CWLVersionMismatch(
                format!("CWL document is not a Workflow: {:?}", workflow_name),
            )));
        };
        let packed = pack_workflow(&workflow, workflow_name, None)?;
        let packed_json = serde_json::to_value(&packed)?;
        std::fs::write("packed.cwl", serde_json::to_string_pretty(&packed)?)?;
        let working_dir = std::env::current_dir()?;
        let graph_json = packed_json
            .get("$graph")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| anyhow!("Missing or invalid '$graph' field"))?;
        export_rocrate(
            output_dir.clone().as_ref(),
            Some(&working_dir.to_string_lossy().to_string()),
            workflow_name,
            run_type,
            Some("local"),
            graph_json,
            None,
        ).await?;
    }

    Ok(())
}

pub async fn execute_local(args: &LocalExecuteArgs) -> Result<(), ExecutionError> {
    if args.is_quiet {
        log::set_max_level(log::LevelFilter::Error);
    }
    if args.podman {
        set_container_engine(ContainerEngine::Podman);
    } else if args.singularity {
        set_container_engine(ContainerEngine::Singularity);
    } else if args.apptainer {
        set_container_engine(ContainerEngine::Apptainer);
    } else {
        set_container_engine(ContainerEngine::Docker);
    }
    let out_dir: Option<PathBuf> = args.out_dir.as_ref().map(PathBuf::from);
    if args.file.is_dir() {
        let ro_crate_meta = args.file.join("ro-crate-metadata.json");
        return execute_cwl_from_rocrate_root(&args.file, out_dir, &ro_crate_meta, &args.rocrate).await.map_err(|e| ExecutionError::Any(anyhow!("{e:#}")));
    } else if args.file.extension().is_some_and(|ext| ext == "zip") {
        let temp_dir = tempfile::tempdir().map_err(ExecutionError::IOError)?;
        let crate_root = unzip_rocrate(&args.file, temp_dir.path()).map_err(ExecutionError::Any)?;
        let ro_crate_meta = crate_root.join("ro-crate-metadata.json");
        return execute_cwl_from_rocrate_root(&crate_root, out_dir, &ro_crate_meta, &args.rocrate).await.map_err(|e| ExecutionError::Any(anyhow!("{e:#}")));
    }
    execute_cwlfile(&args.file, &args.args, out_dir, &args.rocrate).await
}

async fn execute_cwl_from_rocrate_root(crate_root: &Path, out_dir: Option<PathBuf>,
    ro_crate_meta: &Path, rocrate_args: &Option<RocrateArgs>) -> Result<(), ExecutionError> {
    let (cwl_path, input_yaml) = find_cwl_and_yaml_in_rocrate(crate_root)?;
       if !verify_cwl_references(&cwl_path).context("Failed to verify CWL references")? || input_yaml.is_none() {
        let (_tmp, cloned_cwl, cloned_inputs) = clone_from_rocrate_or_cwl(ro_crate_meta, &cwl_path)?;
        let cwl_path_to_run = cloned_cwl
            .as_ref()
            .ok_or_else(|| ExecutionError::Any(anyhow!("Cloned CWL file not found")))?;
        let inputs: Vec<String> = cloned_inputs
            .as_ref()
            .map(|p| vec![p.to_string_lossy().to_string()])
            .unwrap_or_default();
        return execute_cwlfile(cwl_path_to_run, &inputs, out_dir, rocrate_args).await;
    }
    let inputs: Vec<String> = input_yaml.as_ref().map(|p| vec![p.to_string_lossy().to_string()]).unwrap_or_default();
    execute_cwlfile(&cwl_path, &inputs, out_dir, rocrate_args).await
    //execute_cwlfile(&cwl_path, &input_yaml, out_dir, rocrate_args).await
}

pub async fn schedule_run(file: &Path, input_file: &Option<PathBuf>, rocrate_args: &Option<RocrateArgs>, watch: bool, logout: bool) -> Result<(), Box<dyn Error>> {
    let workflow_name = remote_execution::schedule_run(file, input_file).await?;

    if watch {
        remote_execution::watch(&workflow_name, rocrate_args).await?;
    }

    if logout && let Err(e) = remote_execution::logout() {
        eprintln!("Error logging out of reana instance: {e}");
    }

    Ok(())
}

#[allow(clippy::disallowed_macros)]
pub fn make_template(filename: &PathBuf) -> Result<(), Box<dyn Error>> {
    let template = make_template_impl(filename)?;
    let yaml = serde_yaml::to_string(&template)?;

    println!("{yaml}");
    Ok(())
}

fn make_template_impl(filename: &PathBuf) -> Result<HashMap<String, DefaultValue>, Box<dyn Error>> {
    let contents = fs::read_to_string(filename)?;
    let cwl: CWLDocument = serde_yaml::from_str(&contents)?;

    Ok(cwl
        .inputs
        .iter()
        .map(|i| {
            let id = &i.id;
            let dummy_value = if i.default.is_some() {
                return (id.clone(), i.default.clone().unwrap());
            } else {
                match &i.type_ {
                    CWLType::Optional(cwltype) => default_values(cwltype),
                    CWLType::Array(cwltype) => DefaultValue::Any(Value::Sequence(vec![defaults(cwltype), defaults(cwltype)])),
                    cwltype => default_values(cwltype),
                }
            };
            (id.clone(), dummy_value)
        })
        .collect::<HashMap<_, _>>())
}

fn default_values(cwltype: &CWLType) -> DefaultValue {
    match cwltype {
        CWLType::File => DefaultValue::File(File::from_location("./path/to/file.txt")),
        CWLType::Directory => DefaultValue::Directory(Directory::from_location("./path/to/dir")),
        _ => DefaultValue::Any(defaults(cwltype)),
    }
}

fn defaults(cwltype: &CWLType) -> Value {
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
        assert_eq!(defaults(&CWLType::Int), Value::Number(Number::from(42)));
        assert_eq!(defaults(&CWLType::Boolean), Value::Bool(true));
        assert_eq!(defaults(&CWLType::Long), Value::Number(Number::from(42)));
        assert_eq!(defaults(&CWLType::Float), Value::Number(Number::from(69.42)));
        assert_eq!(defaults(&CWLType::String), Value::String("Hello World".into()));
        assert_eq!(defaults(&CWLType::Any), Value::String("Any Value".into()));
    }

    #[test]
    fn test_default_values() {
        assert_eq!(
            default_values(&CWLType::File),
            DefaultValue::File(File::from_location("./path/to/file.txt"))
        );
        assert_eq!(
            default_values(&CWLType::Directory),
            DefaultValue::Directory(Directory::from_location("./path/to/dir"))
        );
        assert_eq!(default_values(&CWLType::String), DefaultValue::Any(Value::String("Hello World".into())));
    }

    #[test]
    fn test_make_template_impl() {
        let path = PathBuf::from("../../testdata/hello_world/workflows/main/main.cwl");
        let template = make_template_impl(&path).unwrap();
        let expected = HashMap::from([
            (
                "population".to_string(),
                DefaultValue::File(File::from_location("../../data/population.csv")),
            ),
            (
                "speakers".to_string(),
                DefaultValue::File(File::from_location("../../data/speakers_revised.csv")),
            ),
        ]);

        assert_eq!(template, expected);
    }
}
