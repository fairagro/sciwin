use clap::Args;
use sciwin::repository::Repository;
use sciwin::repository::{commit, stage_file};
use sciwin::io::get_workflows_folder;
use tracing::info;

#[derive(Args, Debug)]
pub struct SaveArgs {
    #[arg(
        help = "Name of the workflow to be saved",
        value_name = "WORKFLOW_NAME"
    )]
    pub name: String,
}

pub fn save_workflow(args: &SaveArgs) -> anyhow::Result<()> {
    //get workflow
    let filename = format!("{}{}/{}.cwl", get_workflows_folder(), args.name, args.name);
    let repo = Repository::open(".")?;
    stage_file(&repo, &filename)?;
    let msg = &format!("✅ Saved workflow {}", args.name);
    info!("{msg}");
    commit(&repo, msg)?;
    Ok(())
}
