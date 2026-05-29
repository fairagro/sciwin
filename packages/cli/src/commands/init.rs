use anyhow::Context;
use clap::Args;
use log::info;
use std::{env, path::PathBuf};

#[derive(Args, Debug, Default)]
pub struct InitArgs {
    #[arg(short = 'p', long = "project", help = "Name of the project")]
    pub project: Option<String>,
}

pub fn handle_init_command(args: &InitArgs) -> anyhow::Result<()> {
    let base_dir = match &args.project {
        Some(folder) => PathBuf::from(folder),
        None => env::current_dir()?,
    };

    s4n_core::project::initialize_project(&base_dir)
        .inspect_err(|_| {
            let _ = s4n_core::project::git_cleanup(args.project.clone());
        })
        .with_context(|| format!("Could not initialize project at {:?}", base_dir))?;
    info!("📂 Project Initialization successful");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use tempfile::tempdir;
    use test_utils::check_git_user;

    #[test]
    #[serial]
    fn test_init_s4n_without_folder() {
        //create a temp dir
        let temp_dir = tempdir().expect("Failed to create a temporary directory");
        eprintln!("Temporary directory: {:?}", temp_dir.path());
        check_git_user().unwrap();

        // Change current dir to the temporary directory to not create workflow folders etc in sciwin-client dir
        env::set_current_dir(temp_dir.path()).unwrap();
        eprintln!(
            "Current directory changed to: {}",
            env::current_dir().unwrap().display()
        );

        // test method without folder name and do not create arc folders
        let folder_name: Option<String> = None;

        let result = handle_init_command(&InitArgs {
            project: folder_name,
        });

        // Assert results is ok and folders exist/ do not exist
        assert!(result.is_ok());

        assert!(PathBuf::from("workflows").exists());
        assert!(PathBuf::from(".git").exists());
        assert!(PathBuf::from("assays").exists());
        assert!(PathBuf::from("studies").exists());
        assert!(PathBuf::from("runs").exists());
    }
}
