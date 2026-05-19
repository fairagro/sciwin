use std::process::Command;

use commonwl::{
    documents::CommandLineTool, engine::ContainerEngine, files::FileOrDirectory,
    inputs::DefaultValue, requirements::ToolRequirements,
};

pub mod config;
pub mod io;
pub mod parser;
pub mod project;
pub mod tool;
pub mod visualize;
pub mod workflow;

pub fn append_requirement(tool: &mut CommandLineTool, requirement: ToolRequirements) {
    if let Some(reqs) = &mut tool.requirements {
        reqs.push(requirement);
    } else {
        tool.requirements = Some(vec![requirement]);
    }
}

pub fn default_to_string(default: &DefaultValue) -> String {
    match default {
        DefaultValue::FileOrDirectory(FileOrDirectory::File(f)) => f
            .location
            .clone()
            .unwrap_or_else(|| f.path.clone().unwrap()),
        DefaultValue::FileOrDirectory(FileOrDirectory::Directory(d)) => d
            .location
            .clone()
            .unwrap_or_else(|| d.path.clone().unwrap()),
        DefaultValue::Any(value) => value.as_str().unwrap_or_default().to_string(), //??
    }
}

fn command_available(cmd: &str) -> bool {
    #[cfg(unix)]
    let checker = "which";

    #[cfg(windows)]
    let checker = "where";

    Command::new(checker)
        .arg(cmd)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn auto_container_engine() -> Option<ContainerEngine> {
    if command_available("docker") {
        Some(ContainerEngine::Docker)
    } else if command_available("podman") {
        Some(ContainerEngine::Podman)
    } else if command_available("apptainer") {
        Some(ContainerEngine::Apptainer)
    } else if command_available("singularity") {
        Some(ContainerEngine::Singularity)
    } else {
        None
    }
}
