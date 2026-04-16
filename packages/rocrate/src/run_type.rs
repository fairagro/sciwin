use clap::{Args, ValueEnum};

#[derive(Args, Debug, Clone, Default)]
pub struct RocrateArgs {
    #[arg(short = 'n', long = "workflow-name", help = "Workflow name to create a Provenance Run Crate for")]
    pub workflow_name: Option<String>,
    #[arg(short = 'd', long = "rocrate_dir", default_value = "rocrate")]
    pub output_dir: Option<String>,
    #[arg(short = 't', long = "run-type", value_enum, default_value_t = RocrateRunType::ProvenanceRun)]
    pub run_type: RocrateRunType,
}

//added arc rocrate but if user pushes to datahub it is created automatically anyway, also requires arc
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
#[clap(rename_all = "PascalCase")]
pub enum RocrateRunType {
    #[clap(name = "Workflow Run Crate")]
    WorkflowRun,
    #[clap(name = "Process Run Crate")]
    ProcessRun,
    #[clap(name = "Workflow RO-Crate")]
    WorkflowROCrate,
    #[clap(name = "ARC RO-Crate")]
    ArcROCrate,
    #[default]
    #[clap(name = "Provenance Run Crate")]
    ProvenanceRun,
}