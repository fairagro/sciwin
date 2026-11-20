use crate::cwl::workflow_filename;
use clap::Args;
use miette::IntoDiagnostic;
use sciwin::repository::Repository;
use sciwin::repository::{commit, stage_file};
use std::path::Path;
use tracing::{debug, info};

#[derive(Args, Debug)]
pub struct SaveArgs {
    #[arg(
        help = "Name of the workflow to be saved, or a path to its .cwl file",
        value_name = "WORKFLOW"
    )]
    pub name: String,
}

pub fn stage_and_commit_workflow(args: &SaveArgs) -> miette::Result<()> {
    let filename = workflow_filename(&args.name);
    if !Path::new(&filename).is_file() {
        miette::bail!("No workflow file at {filename}");
    }
    debug!("staging {filename:?} for commit");
    let repo = Repository::open(".").into_diagnostic()?;
    stage_file(&repo, &filename)?;
    let msg = &format!("✅ Saved workflow {}", args.name);
    info!("{msg}");
    commit(&repo, msg)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use fstest::fstest;
    use std::fs;

    #[fstest(repo = true)]
    fn saves_a_workflow_given_by_path() {
        fs::create_dir_all("pipelines").unwrap();
        fs::write(
            "pipelines/demo.cwl",
            "cwlVersion: v1.2\nclass: Workflow\ninputs: []\noutputs: []\nsteps: []\n",
        )
        .unwrap();

        let name = "pipelines/demo.cwl".to_string();
        stage_and_commit_workflow(&SaveArgs { name }).unwrap();

        let repo = Repository::open(".").unwrap();
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        assert_eq!(
            head.message().unwrap(),
            "✅ Saved workflow pipelines/demo.cwl"
        );
        let status = repo.status_file(Path::new("pipelines/demo.cwl")).unwrap();
        assert!(status.is_empty(), "file not committed: {status:?}");
    }

    #[fstest(repo = true)]
    fn missing_workflow_is_an_error() {
        let name = "nope".to_string();
        let err = stage_and_commit_workflow(&SaveArgs { name }).unwrap_err();
        assert!(err.to_string().contains("nope.cwl"), "{err}");
    }
}
