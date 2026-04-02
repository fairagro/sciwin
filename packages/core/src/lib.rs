use commonwl::{documents::CommandLineTool, requirements::ToolRequirements};

pub mod config;
pub mod io;
pub mod parser;
pub mod project;
pub mod tool;
pub mod visualize;
pub mod workflow;

pub(crate) fn append_requirement(tool: &mut CommandLineTool, requirement: ToolRequirements) {
    if let Some(reqs) = &mut tool.requirements {
        reqs.push(requirement);
    } else {
        tool.requirements = Some(vec![requirement]);
    }
}
