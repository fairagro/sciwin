use clap::{Args, ValueEnum};
use miette::miette;
use sciwin::cwl::{documents::CWLDocument, load_cwl_file};
use sciwin::visualize::{DotRenderer, MermaidRenderer, render};
use std::path::PathBuf;

#[derive(Args, Debug)]
pub struct VisualizeWorkflowArgs {
    #[arg(help = "Path to a workflow")]
    pub filename: PathBuf,
    #[arg(short = 'r', long = "renderer", help = "Select a flavor", value_enum, default_value_t = Renderer::Mermaid)]
    pub renderer: Renderer,
    #[arg(
        long = "no-defaults",
        help = "Do not print default values",
        default_value_t = false
    )]
    pub no_defaults: bool,
}

#[derive(Default, Debug, Clone, ValueEnum)]
pub enum Renderer {
    #[default]
    Mermaid,
    Dot,
}

#[allow(clippy::disallowed_macros)]
pub fn visualize(filename: &PathBuf, renderer: &Renderer, no_defaults: bool) -> miette::Result<()> {
    let cwl = load_cwl_file(filename, true)
        .map_err(|e| miette!("Could mot load Workflow {filename:?}: {e}"))?;
    let CWLDocument::Workflow(cwl) = cwl else {
        return Err(miette!(
            "The provided file {filename:?} does not contain a CWL Workflow"
        ));
    };

    let code = match renderer {
        Renderer::Dot => render(&mut DotRenderer::default(), &cwl, filename, no_defaults),
        Renderer::Mermaid => render(&mut MermaidRenderer::default(), &cwl, filename, no_defaults),
    }
    .map_err(|e| {
        miette!("Could not render visualization for {filename:?} using {renderer:?}: {e}")
    })?;

    println!("{code}");
    Ok(())
}
