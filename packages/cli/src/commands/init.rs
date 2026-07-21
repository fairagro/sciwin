use anyhow::Context;
use clap::Args;
use tracing::info;
use std::path::PathBuf;

#[derive(Args, Debug, Default)]
pub struct InitArgs {
    #[arg(short = 'p', long = "project", help = "Name of the project")]
    pub project: Option<String>,
}

pub fn handle_init_command(args: &InitArgs) -> anyhow::Result<()> {
    let base_dir = match &args.project {
        Some(folder) => PathBuf::from(folder),
        None => PathBuf::new(),
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
    use std::env;
    use tempfile::tempdir;
    use test_utils::check_git_user;

    #[test]
    #[serial]
    fn test_init_s4n_without_folder() {
        let temp_dir = tempdir().expect("Failed to create a temporary directory");
        let cwd = env::current_dir().unwrap();

        eprintln!("Temporary directory: {:?}", temp_dir.path());
        check_git_user().unwrap();

        env::set_current_dir(temp_dir.path()).unwrap();
        eprintln!(
            "Current directory changed to: {}",
            env::current_dir().unwrap().display()
        );

        let folder_name: Option<String> = None;

        let result = handle_init_command(&InitArgs {
            project: folder_name,
        });

        assert!(result.is_ok());

        assert!(PathBuf::from("workflows").exists());
        assert!(PathBuf::from(".git").exists());

        env::set_current_dir(cwd).unwrap();
    }
}
