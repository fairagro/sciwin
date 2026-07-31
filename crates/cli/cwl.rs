use dialoguer::{Select, theme::ColorfulTheme};
use sciwin::authoring::paths::WORKFLOWS_FOLDER;
use sciwin::repository::Repository;
use sciwin::repository::submodule::get_submodule_paths;
use std::{error::Error, path::PathBuf};
use syntect::{
    easy::HighlightLines,
    highlighting::ThemeSet,
    parsing::SyntaxSet,
    util::{LinesWithEndings, as_24_bit_terminal_escaped},
};

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
            let items: Vec<String> = candidates
                .iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect();
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
