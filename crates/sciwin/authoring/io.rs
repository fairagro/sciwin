use crate::authoring::parser::{SCRIPT_EXECUTORS, SCRIPT_MODIFIERS};
use commonwl::OneOrMany;
use std::path::Path;

pub fn get_workflows_folder() -> String {
    "workflows/".to_string()
}
/// Derives the tool's base name (no extension) from its command line, or from `the_name`
pub fn derive_tool_name(command: &OneOrMany<String>, the_name: Option<&str>) -> String {
    let mut filename = match &command {
        OneOrMany::Many(cmd) => {
            if cmd.len() > 2 && SCRIPT_EXECUTORS.contains(&cmd[0].as_str()) && SCRIPT_MODIFIERS.contains(&cmd[1].as_str()) {
                get_filename_without_extension(cmd[2].as_str())
            } else if SCRIPT_EXECUTORS.contains(&cmd[0].as_str()) {
                get_filename_without_extension(cmd[1].as_str())
            } else {
                get_filename_without_extension(cmd[0].as_str())
            }
        }
        OneOrMany::One(cmd) => get_filename_without_extension(cmd.as_str()),
    };

    filename = Path::new(&filename).file_name().unwrap_or_default().to_string_lossy().into_owned();

    if let Some(name) = the_name {
        filename = name.to_string();
        if is_cwl_file(&filename) {
            filename = filename.replace(".cwl", "");
        }
    }

    filename
}

/// Builds `{base_dir}/{tool_name}.cwl` for the given command/name. 
pub fn get_qualified_filename(
    command: &OneOrMany<String>,
    the_name: Option<&str>,
    base_dir: impl AsRef<Path>,
) -> String {
    let filename = format!("{}.cwl", derive_tool_name(command, the_name));
    base_dir.as_ref().join(filename).to_string_lossy().into_owned()
}

fn is_cwl_file(path: &str) -> bool {
    Path::new(path).extension().is_some_and(|ext| ext.eq_ignore_ascii_case("cwl"))
}

pub(crate) fn get_filename_without_extension(relative_path: impl AsRef<Path>) -> String {
    let filename = relative_path
        .as_ref()
        .file_name()
        .map(|f| f.to_string_lossy())
        .unwrap_or(relative_path.as_ref().to_string_lossy());
    filename.split('.').next().unwrap_or(&filename).to_string()
}

pub(crate) fn resolve_path<P: AsRef<Path>, Q: AsRef<Path>>(filename: P, relative_to: Q) -> String {
    let path = filename.as_ref();
    let relative_path = Path::new(relative_to.as_ref());
    let base_dir = match relative_path.extension() {
        Some(_) => relative_path.parent().unwrap_or_else(|| Path::new(".")),
        None => relative_path,
    };

    // pathdiff can't relativize across roots (e.g. different drives on Windows) --
    // fall back to the original path unchanged rather than panicking.
    pathdiff::diff_paths(path, base_dir)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;
    use test_utils::os_path;

    #[rstest]
    #[case("results.csv", "results")]
    #[case("/some/relative/path.txt", "path")]
    #[case("some/archive.tar.gz", "archive")]
    pub fn test_get_filename_without_extension(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(get_filename_without_extension(input), expected.to_string());
    }

    #[rstest]
    #[case("tests/testdata/input.txt", "workflows/echo/echo.cwl", "../../tests/testdata/input.txt")]
    #[case("tests/testdata/input.txt", "workflows/echo/", "../../tests/testdata/input.txt")]
    #[case("workflows/echo/echo.py", "workflows/echo/echo.cwl", "echo.py")]
    #[case("workflows/lol/echo.py", "workflows/echo/echo.cwl", "../lol/echo.py")]
    #[case("/home/user/workflows/echo/echo.py", "/home/user/workflows/echo/echo.cwl", "echo.py")]
    fn test_resolve_path(#[case] path: &str, #[case] relative_to: &str, #[case] expected: &str) {
        assert_eq!(resolve_path(path, relative_to), os_path(expected));
    }

    #[test]
    pub fn test_get_workflows_folder() {
        //could be variable in future
        assert_eq!(get_workflows_folder(), "workflows/");
    }

    #[rstest]
    #[case(OneOrMany::Many(vec!["python3".to_string(), "test/data/script.py".to_string()]), "script")]
    #[case(OneOrMany::One("echo".to_string()), "echo")]
    fn test_derive_tool_name(#[case] command: OneOrMany<String>, #[case] expected: &str) {
        assert_eq!(derive_tool_name(&command, None), expected);
    }

    #[test]
    fn test_derive_tool_name_with_name() {
        assert_eq!(
            derive_tool_name(&OneOrMany::One("echo".to_string()), Some("hello")),
            "hello"
        );
    }

    #[rstest]
    #[case(OneOrMany::Many(vec!["python3".to_string(), "test/data/script.py".to_string()]), "workflows/script.cwl")]
    #[case(OneOrMany::One("echo".to_string()), "workflows/echo.cwl")]
    fn test_get_qualified_filename(#[case] command: OneOrMany<String>, #[case] expected: &str) {
        assert_eq!(
            get_qualified_filename(&command, None, get_workflows_folder()),
            expected
        );
    }

    #[test]
    fn test_get_qualified_filename_with_name() {
        assert_eq!(
            get_qualified_filename(
                &OneOrMany::One("echo".to_string()),
                Some("hello"),
                get_workflows_folder()
            ),
            "workflows/hello.cwl"
        );
    }

    #[test]
    fn test_get_qualified_filename_custom_base_dir() {
        // the base directory is entirely the caller's decision, not derived here
        assert_eq!(
            get_qualified_filename(&OneOrMany::One("echo".to_string()), None, "out/dir"),
            "out/dir/echo.cwl"
        );
    }

    #[test]
    fn test_get_qualified_filename_shared_folder_for_multiple_tools() {
        // ARC-style grouping: caller points several tools (and a subworkflow) at
        // the same folder instead of getting a fresh folder per tool forced on them
        let shared = "workflows/my_group";
        assert_eq!(
            get_qualified_filename(&OneOrMany::One("tool_a".to_string()), None, shared),
            "workflows/my_group/tool_a.cwl"
        );
        assert_eq!(
            get_qualified_filename(&OneOrMany::One("tool_b".to_string()), None, shared),
            "workflows/my_group/tool_b.cwl"
        );
    }
}
