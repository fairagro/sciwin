use clap::Args;
use std::path::PathBuf;
use tracing::{debug, info};

#[derive(Args, Debug, Default)]
pub struct InitArgs {
    #[arg(short = 'p', long = "project", help = "Name of the project")]
    pub project: Option<String>,
}

pub fn handle_init_command(args: &InitArgs) -> miette::Result<()> {
    use miette::Context;

    let base_dir = match &args.project {
        Some(folder) => PathBuf::from(folder),
        None => PathBuf::new(),
    };
    debug!(
        "initializing project scaffold (folder structure + git repo) at {:?}",
        base_dir
    );

    sciwin::project::initialize_project(&base_dir)
        .inspect_err(|_| {
            debug!("initialization failed, cleaning up partially created git repo");
            let _ = sciwin::project::git_cleanup(args.project.clone());
        })
        .with_context(|| format!("Could not initialize project at {:?}", base_dir))?;
    info!("📂 Project Initialization successful");
    Ok(())
}
