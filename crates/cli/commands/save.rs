use clap::Args;
use sciwin::authoring::paths::{WORKFLOWS_FOLDER, get_qualified_filename_by_name};
use sciwin::repository::Repository;
use sciwin::repository::{commit, stage_file};
use std::path::Path;
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
    let filename = get_qualified_filename_by_name(&args.name, Path::new(WORKFLOWS_FOLDER).join(&args.name));
    let repo = Repository::open(".")?;
    stage_file(&repo, &filename)?;
    let msg = &format!("✅ Saved workflow {}", args.name);
    info!("{msg}");
    commit(&repo, msg)?;
    Ok(())
}
