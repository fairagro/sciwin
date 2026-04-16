use clap::Args;
use colored::Colorize;
use log::info;
use repository::{
    Repository,
    submodule::{add_submodule, remove_submodule},
};
use reqwest::Url;
use std::{
    env,
    path::{Path, PathBuf},
};

#[derive(Args, Debug)]
pub struct InstallPackageArgs {
    #[arg(value_name = "PACKAGE_IDENTIFIER", required = false)]
    pub identifier: String,
    #[arg(short = 'b', long = "branch", help = "Specify branch or commit")]
    pub branch: Option<String>,
}

#[derive(Args, Debug)]
pub struct PackageArgs {
    #[arg(value_name = "PACKAGE_IDENTIFIER", required = false)]
    pub identifier: String,
}

pub fn install_package(url: &str, branch: &Option<String>) -> anyhow::Result<()> {
    let url = if url.starts_with("http") {
        url
    } else {
        &format!("https://github.com/{url}")
    };
    let url = url.strip_suffix(".git").unwrap_or(url);

    let url_obj = Url::parse(url)?;

    let package_dir = Path::new("packages");
    let repo_name = url_obj.path().strip_prefix("/").unwrap();

    let current_dir = env::current_dir().unwrap_or(PathBuf::from("."));
    let mut repo = Repository::open(&current_dir)?;

    add_submodule(&mut repo, url, branch, &package_dir.join(repo_name))?;

    info!("📦 Installed Package {}", repo_name.bold().green());

    Ok(())
}

pub fn remove_package(package_id: &str) -> anyhow::Result<()> {
    let current_dir = env::current_dir().unwrap_or(PathBuf::from("."));
    let repo = Repository::open(&current_dir)?;

    remove_submodule(&repo, &format!("packages/{package_id}"))?;

    info!("📦 Removed Package {}", package_id.bold().red());
    Ok(())
}
