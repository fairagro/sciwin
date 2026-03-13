use clap::{Args, Subcommand};
use commonwl::execution::error::ExecutionError;
use commonwl::execution::{ContainerEngine, execute_cwlfile, set_container_engine};
use commonwl::prelude::*;
use remote_execution::{check_status, download_results, export_rocrate, logout};
use serde_yaml::{Number, Value};
use std::{collections::HashMap, error::Error, fs, path::{Path, PathBuf}};

pub fn handle_execute_commands(subcommand: &ExecuteCommands) -> Result<(), Box<dyn Error>> {
    match subcommand {
        ExecuteCommands::Local(args) => execute_local(args).map_err(|e| Box::new(e).into()),
        ExecuteCommands::Remote(remote_args) => match &remote_args.command {
            RemoteSubcommands::Start {
                file,
                input_file,
                rocrate,
                watch,
                logout,
            } => schedule_run(file, input_file, *rocrate, *watch, *logout),
            RemoteSubcommands::Status { workflow_name } => check_status(workflow_name),
            RemoteSubcommands::Download { workflow_name, all, output_dir } => download_results(workflow_name, *all, output_dir.as_ref()),
            RemoteSubcommands::Rocrate { workflow_name, output_dir } => export_rocrate(workflow_name, output_dir.as_ref(), None),
            RemoteSubcommands::Logout => logout(),
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
    pub out_dir: Option<String>,
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
    #[arg(trailing_var_arg = true, help = "Other arguments provided to cwl file", allow_hyphen_values = true)]
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

pub fn execute_local(args: &LocalExecuteArgs) -> Result<(), ExecutionError> {
    if args.is_quiet {
        log::set_max_level(log::LevelFilter::Error);
    }
    if args.podman {
        set_container_engine(ContainerEngine::Podman);
    }
    else if args.singularity {
        set_container_engine(ContainerEngine::Singularity);
    } 
    else if args.apptainer {
        set_container_engine(ContainerEngine::Apptainer);
    }
    else {
        set_container_engine(ContainerEngine::Docker);
    }
    execute_cwlfile(&args.file, &args.args, args.out_dir.clone())
}

pub fn schedule_run(file: &Path, input_file: &Option<PathBuf>, rocrate: bool, watch: bool, logout: bool) -> Result<(), Box<dyn Error>> {
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
