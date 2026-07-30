use commonwl::{
    documents::CommandLineTool, files::FileOrDirectory, inputs::DefaultValue,
    requirements::ToolRequirements,
};
use miette::Diagnostic;
use thiserror::Error;

pub mod parser;

pub type AuthoringResult<T> = Result<T, AuthoringError>;

#[derive(Error, Diagnostic, Debug)]
pub enum AuthoringError {
    #[error(transparent)]
    #[diagnostic(code = "io::Error")]
    IO(#[from] std::io::Error),

    #[error(transparent)]
    #[diagnostic(code = "serde_json::Error")]
    Json(#[from] serde_json::Error),

    #[error(transparent)]
    #[diagnostic(code = "serde_saphyr::Error")]
    Yaml(#[from] serde_saphyr::Error),
}

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
        DefaultValue::Any(value) => value.as_str().unwrap_or_default().to_string(),
    }
}
