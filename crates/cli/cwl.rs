use crate::interactive::{Prompt, prompt_mode};
use dialoguer::{Select, theme::ColorfulTheme};
use sciwin::authoring::paths::{WORKFLOWS_FOLDER, get_qualified_filename_by_name};
use sciwin::repository::Repository;
use sciwin::repository::submodule::get_submodule_paths;
use std::{
    error::Error,
    path::{Path, PathBuf},
};
use syntect::{
    easy::HighlightLines,
    highlighting::ThemeSet,
    parsing::SyntaxSet,
    util::{LinesWithEndings, as_24_bit_terminal_escaped},
};
use tracing::{debug, warn};

pub fn has_cwl_extension(name: &str) -> bool {
    Path::new(name)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("cwl"))
}

/// The workflow file `name` refers to: itself if it ends in `.cwl`, else
/// `workflows/<name>/<name>.cwl`.
pub fn workflow_filename(name: &str) -> String {
    if has_cwl_extension(name) {
        return name.to_string();
    }
    get_qualified_filename_by_name(name, Path::new(WORKFLOWS_FOLDER).join(name))
        .to_string_lossy()
        .into_owned()
}

/// Locates CWL File by name
pub fn resolve_filename(cwl_filename: &str) -> Result<String, Box<dyn Error>> {
    let mut candidates: Vec<PathBuf> = vec![];

    //check if exists in workflows folder
    if let Some(path) = build_path(None, cwl_filename) {
        debug!("candidate in top-level workflows folder: {}", path.display());
        candidates.push(path);
    }

    //let else = hell yeah!
    let Ok(repo) = Repository::open(".") else {
        debug!("not inside a git repository, skipping submodule search");
        if !candidates.is_empty() {
            return Ok(candidates[0].to_string_lossy().into_owned());
        }
        return Err("No candidates available".into());
    };

    for module_path in get_submodule_paths(&repo)? {
        if let Some(path) = build_path(Some(module_path), cwl_filename) {
            debug!("candidate in submodule: {}", path.display());
            candidates.push(path);
        }
    }

    debug!("{} candidate(s) found for '{cwl_filename}'", candidates.len());
    match candidates.len() {
        1 => Ok(candidates[0].to_string_lossy().into_owned()),
        0 => Err("Could not resolve filename".into()),
        _ => {
            let items: Vec<String> = candidates
                .iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect();
            match prompt_mode() {
                Prompt::Ask => {
                    let selection = Select::with_theme(&ColorfulTheme::default())
                        .with_prompt("Multiple candidates are found. Select the CWL File to use")
                        .items(&items)
                        .default(0)
                        .report(true)
                        .interact()?;
                    Ok(items[selection].clone())
                }
                Prompt::AssumeYes => {
                    warn!(
                        "'{cwl_filename}' is ambiguous, using {} (other candidates: {})",
                        items[0],
                        items[1..].join(", ")
                    );
                    Ok(items[0].clone())
                }
                Prompt::Unavailable => Err(format!(
                    "'{cwl_filename}' is ambiguous: {}. Pass --yes to use the first one",
                    items.join(", ")
                )
                .into()),
            }
        }
    }
}

fn build_path(base: Option<PathBuf>, cwl_filename: &str) -> Option<PathBuf> {
    let path = base.unwrap_or_default();

    let cwl_filename = cwl_filename.strip_suffix(".cwl").unwrap_or(cwl_filename);

    let candidate_1 = path
        .join(WORKFLOWS_FOLDER)
        .join(cwl_filename)
        .join(format!("{cwl_filename}.cwl"));
    let candidate_2 = path
        .join(WORKFLOWS_FOLDER)
        .join(cwl_filename)
        .join("workflow.cwl");

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
    use fstest::fstest;
    use std::{
        env,
        path::{MAIN_SEPARATOR, Path},
    };

    #[fstest(repo = true, files = ["../../testdata/input.txt", "../../testdata/echo.py"])]
    fn test_resolve_filename_in_submodule() {
        let repo = Repository::open(env::current_dir().unwrap()).unwrap();
        let mut module = repo
            .submodule(
                "https://github.com/fairagro/M4.4_UC6_ARC",
                Path::new("uc6"),
                false,
            )
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
                "{}{MAIN_SEPARATOR}{WORKFLOWS_FOLDER}{MAIN_SEPARATOR}{name}{MAIN_SEPARATOR}{name}.cwl",
                module.path().to_string_lossy(),
            )
        );
    }
}
