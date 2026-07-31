use clap::Args;
use sciwin::repository::Repository;
use sciwin::repository::{commit, stage_file};
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
    let filename = format!("workflows/{}/{}.cwl", args.name, args.name); // todo: fix
    let repo = Repository::open(".")?;
    stage_file(&repo, &filename)?;
    let msg = &format!("✅ Saved workflow {}", args.name);
    info!("{msg}");
    commit(&repo, msg)?;
    Ok(())
}
