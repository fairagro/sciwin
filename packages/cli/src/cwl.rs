use commonwl::{CWLDocument, Workflow};
use dialoguer::{Select, theme::ColorfulTheme};
use log::info;
use repository::Repository;
use repository::submodule::get_submodule_paths;
use s4n_core::io::get_workflows_folder;
use std::{error::Error, path::PathBuf};
use syntect::{
    easy::HighlightLines,
    highlighting::ThemeSet,
    parsing::SyntaxSet,
    util::{LinesWithEndings, as_24_bit_terminal_escaped},
};

pub trait Connectable {
    fn remove_output_connection(&mut self, from: &str, to_output: &str) -> Result<(), Box<dyn Error>>;
    fn remove_input_connection(&mut self, from_input: &str, to: &str) -> Result<(), Box<dyn Error>>;
    fn add_step_connection(&mut self, from: &str, to: &str) -> Result<(), Box<dyn Error>>;
    fn add_output_connection(&mut self, from: &str, to_output: &str) -> Result<(), Box<dyn Error>>;
    fn add_input_connection(&mut self, from_input: &str, to: &str) -> Result<(), Box<dyn Error>>;
    fn add_new_step_if_not_exists(&mut self, name: &str, path: &str, doc: &CWLDocument);
    fn remove_step_connection(&mut self, from: &str, to: &str) -> Result<(), Box<dyn Error>>;
}

impl Connectable for Workflow {
    fn add_new_step_if_not_exists(&mut self, name: &str, path: &str, doc: &CWLDocument) {
        s4n_core::workflow::add_workflow_step(self, name, path, doc);
        info!("➕ Added step {name} to workflow");
    }

    /// Adds a connection between an input and a `CommandLineTool`. The tool will be registered as step if it is not already and an Workflow input will be added.
    fn add_input_connection(&mut self, from_input: &str, to: &str) -> Result<(), Box<dyn Error>> {
        let to_parts = to.split('/').collect::<Vec<_>>();
        let to_filename = resolve_filename(to_parts[0])?;

        s4n_core::workflow::add_workflow_input_connection(self, from_input, &to_filename, to_parts[0], to_parts[1])?;
        info!("➕ Added or updated connection from inputs.{from_input} to {to} in workflow");

        Ok(())
    }

    /// Adds a connection between an output and a `CommandLineTool`. The tool will be registered as step if it is not already and an Workflow output will be added.
    fn add_output_connection(&mut self, from: &str, to_output: &str) -> Result<(), Box<dyn Error>> {
        let from_parts = from.split('/').collect::<Vec<_>>();
        let from_filename = resolve_filename(from_parts[0])?;

        s4n_core::workflow::add_workflow_output_connection(self, from_parts[0], from_parts[1], &from_filename, to_output)?;
        info!("➕ Added or updated connection from {from} to outputs.{to_output} in workflow!");

        Ok(())
    }

    /// Adds a connection between two `CommandLineTools`. The tools will be registered as step if registered not already.
    fn add_step_connection(&mut self, from: &str, to: &str) -> Result<(), Box<dyn Error>> {
        //handle from
        let from_parts = from.split('/').collect::<Vec<_>>();
        let from_filename = resolve_filename(from_parts[0])?;
        //handle to
        let to_parts = to.split('/').collect::<Vec<_>>();
        let to_filename = resolve_filename(to_parts[0])?;

        s4n_core::workflow::add_workflow_step_connection(self, &from_filename, from_parts[0], from_parts[1], &to_filename, to_parts[0], to_parts[1])?;
        info!("🔗 Added connection from {from} to {to} in workflow!");

        Ok(())
    }

    /// Removes a connection between two `CommandLineTools` by removing input from `tool_y` that is also output of `tool_x`.
    fn remove_step_connection(&mut self, from: &str, to: &str) -> Result<(), Box<dyn Error>> {
        let from_parts = from.split('/').collect::<Vec<_>>();
        let to_parts = to.split('/').collect::<Vec<_>>();
        if from_parts.len() != 2 {
            return Err(format!("Invalid '--from' format: {from}. Please use tool/parameter or @inputs/parameter.").into());
        }
        if to_parts.len() != 2 {
            return Err(format!("Invalid '--to' format: {to}. Please use tool/parameter or @outputs/parameter.").into());
        }
        if !self.has_step(to_parts[0]) {
            return Err(format!("Step {} not found!", to_parts[0]).into());
        }

        s4n_core::workflow::remove_workflow_step_connection(self, to_parts[0], to_parts[1])?;
        info!("➖ Removed connection from {from} to {to} in workflow!");
        Ok(())
    }

    /// Removes an input from inputs and removes it from `CommandLineTool` input.
    fn remove_input_connection(&mut self, from_input: &str, to: &str) -> Result<(), Box<dyn Error>> {
        let to_parts = to.split('/').collect::<Vec<_>>();
        if to_parts.len() != 2 {
            return Err(format!("Invalid 'to' format for input connection: {from_input} to:{to}").into());
        }

        s4n_core::workflow::remove_workflow_input_connection(self, from_input, to_parts[0], to_parts[1], true)?;
        info!("➖ Removed connection from inputs.{from_input} to {to} in workflow");
        Ok(())
    }

    /// Removes a connection between an output and a `CommandLineTool`.
    fn remove_output_connection(&mut self, _from: &str, to_output: &str) -> Result<(), Box<dyn Error>> {
        s4n_core::workflow::remove_workflow_output_connection(self, to_output, true)?;
        info!("➖ Removed connection to {to_output} from workflow!");
        Ok(())
    }
}

/// Locates CWL File by name
pub fn resolve_filename(cwl_filename: &str) -> Result<String, Box<dyn Error>> {
    let mut candidates: Vec<PathBuf> = vec![];

    //check if exists in workflows folder
    if let Some(path) = build_path(None, cwl_filename) {
        candidates.push(path);
    }

    //let else = hell yeah!
    let Ok(repo) = Repository::open(".") else {
        if !candidates.is_empty() {
            return Ok(candidates[0].to_string_lossy().into_owned());
        }
        return Err("No candidates available".into());
    };

    for module_path in get_submodule_paths(&repo)? {
        if let Some(path) = build_path(Some(module_path), cwl_filename) {
            candidates.push(path);
        }
    }

    match candidates.len() {
        1 => Ok(candidates[0].to_string_lossy().into_owned()),
        0 => Err("Could not resolve filename".into()),
        _ => {
            let items: Vec<String> = candidates.iter().map(|p| p.to_string_lossy().into_owned()).collect();
            let selection = Select::with_theme(&ColorfulTheme::default())
                .with_prompt("Multiple candidates are found. Select the CWL File to use")
                .items(&items)
                .default(0)
                .report(true)
                .interact()?;
            Ok(items[selection].clone())
        }
    }
}

fn build_path(base: Option<PathBuf>, cwl_filename: &str) -> Option<PathBuf> {
    let path = base.unwrap_or_default();
    let wf_folder = get_workflows_folder();

    let cwl_filename = cwl_filename.strip_suffix(".cwl").unwrap_or(cwl_filename);

    let candidate_1 = path.join(&wf_folder).join(cwl_filename).join(format!("{cwl_filename}.cwl"));
    let candidate_2 = path.join(&wf_folder).join(cwl_filename).join("workflow.cwl");

    candidate_1
        .exists()
        .then_some(candidate_1)
        .or_else(|| candidate_2.exists().then_some(candidate_2))
}

#[allow(clippy::disallowed_macros)]
pub fn highlight_cwl(yaml: &str) {
    let ps = SyntaxSet::load_defaults_newlines();
    let ts = ThemeSet::load_defaults();

    let syntax = ps.find_syntax_by_extension("yaml").unwrap();
    let mut h = HighlightLines::new(syntax, &ts.themes["InspiredGitHub"]);

    for line in LinesWithEndings::from(yaml) {
        let ranges = h.highlight_line(line, &ps).unwrap();
        let escaped = as_24_bit_terminal_escaped(&ranges[..], false);
        print!("{escaped}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::{CreateArgs, create_tool};
    use fstest::fstest;
    use s4n_core::io::get_workflows_folder;
    use std::{
        env,
        path::{MAIN_SEPARATOR, Path},
    };

    #[fstest(repo = true, files = ["../../testdata/input.txt", "../../testdata/echo.py"])]
    fn test_resolve_filename() {
        create_tool(&CreateArgs {
            command: vec![
                "python3".to_string(),
                "echo.py".to_string(),
                "--test".to_string(),
                "input.txt".to_string(),
            ],
            ..Default::default()
        })
        .unwrap();

        let name = "echo";
        let path = resolve_filename(name).unwrap();
        assert_eq!(path, format!("{}{name}{MAIN_SEPARATOR}{name}.cwl", get_workflows_folder()));
    }

    #[fstest(repo = true, files = ["../../testdata/input.txt", "../../testdata/echo.py"])]
    fn test_resolve_filename_in_submodule() {
        let repo = Repository::open(env::current_dir().unwrap()).unwrap();
        let mut module = repo
            .submodule("https://github.com/fairagro/M4.4_UC6_ARC", Path::new("uc6"), false)
            .unwrap();
        module.init(false).unwrap();
        let subrepo = module.open().unwrap();

        subrepo
            .find_remote("origin")
            .unwrap()
            .fetch(&["refs/heads/*:refs/remotes/origin/*"], None, None)
            .unwrap();
        subrepo.set_head("refs/remotes/origin/main").unwrap();
        subrepo.checkout_head(None).unwrap();
        module.add_finalize().unwrap();

        let name = "get_soil_data";
        let path = resolve_filename(name).unwrap();
        assert_eq!(
            path,
            format!(
                "{}{MAIN_SEPARATOR}{}{name}{MAIN_SEPARATOR}{name}.cwl",
                module.path().to_string_lossy(),
                get_workflows_folder()
            )
        );
    }
}
