use crate::commands::{
    AnnotateCommands, ConnectWorkflowArgs, CreateArgs, ExecuteCommands, InitArgs, InstallPackageArgs, ListCWLArgs, PackageArgs, RemoveCWLArgs, SaveArgs, VisualizeWorkflowArgs
};
use clap::{Command, Parser, Subcommand};
use clap_complete::{Generator, Shell, generate};
use std::io;

#[derive(Parser, Debug)]
#[command(name="s4n", about=format!(r#"
 _____        _  _    _  _____         _____  _  _               _   
/  ___|      (_)| |  | ||_   _|       /  __ \| |(_)             | |   
\ `--.   ___  _ | |  | |  | |  _ __   | /  \/| | _   ___  _ __  | |_  
 `--. \ / __|| || |/\| |  | | | '_ \  | |    | || | / _ \| '_ \ | __|
/\__/ /| (__ | |\  /\  / _| |_| | | | | \__/\| || ||  __/| | | || |_  
\____/  \___||_| \/  \/  \___/|_| |_|  \____/|_||_| \___||_| |_| \__|

Client tool for Scientific Workflow Infrastructure (SciWIn)
Documentation: https://fairagro.github.io/sciwin/

Version: {}"#, env!("CARGO_PKG_VERSION"))
, long_about=None, version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    #[command(about = "Initializes project folder structure and repository")]
    Init(InitArgs),
    #[command(about = "Creates a new CWL File or Workflow")]
    Create(CreateArgs),
    #[command(about = "Lists either all CWL Files or details to a given file", visible_alias = "ls")]
    List(ListCWLArgs),
    #[command(about = "Removes a CWL File from the workflows Directory", visible_alias = "rm")]
    Remove(RemoveCWLArgs),
    #[command(about = "Connects a workflow node")]
    Connect(ConnectWorkflowArgs),
    #[command(about = "Disconnects a workflow node")]
    Disconnect(ConnectWorkflowArgs),
    #[command(about = "Visualizes a workflow")]
    Visualize(VisualizeWorkflowArgs),
    #[command(about = "Saves a workflow")]
    Save(SaveArgs),
    #[command(about = "Installs a workflow as submodule", visible_alias = "i")]
    Install(InstallPackageArgs),
    #[command(about = "Removes an installed workflow")]
    Uninstall(PackageArgs),
    #[command(about = "Execution of CWL Files locally or on remote servers", visible_alias = "ex")]
    Execute {
        #[command(subcommand)]
        command: ExecuteCommands,
    },
    #[command(about = "Annotate CWL files")]
    Annotate {
        #[command(subcommand)]
        command: Option<AnnotateCommands>,
        #[arg(value_name = "TOOL_NAME", required = false)]
        tool_name: Option<String>,
    },
    #[command(about = "Generate shell completions")]
    Completions {
        #[arg()]
        shell: Shell,
    },
}

pub fn generate_completions<G: Generator>(generator: G, cmd: &mut Command) -> anyhow::Result<()> {
    generate(generator, cmd, cmd.get_name().to_string(), &mut io::stdout());
    Ok(())
}
