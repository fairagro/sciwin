use crate::parser::{SCRIPT_EXECUTORS, SCRIPT_MODIFIERS};
use commonwl::Command;
use std::path::Path;
use util::is_cwl_file;

pub fn get_workflows_folder() -> String {
    "workflows/".to_string()
}
pub fn get_qualified_filename(command: &Command, the_name: Option<String>) -> String {
    //decide over filename

    let mut filename = match &command {
        Command::Multiple(cmd) => {
            if cmd.len() > 2 && SCRIPT_EXECUTORS.contains(&cmd[0].as_str()) && SCRIPT_MODIFIERS.contains(&cmd[1].as_str()) {
                get_filename_without_extension(cmd[2].as_str())
            } else if SCRIPT_EXECUTORS.contains(&cmd[0].as_str()) {
                get_filename_without_extension(cmd[1].as_str())
            } else {
                get_filename_without_extension(cmd[0].as_str())
            }
        }
        Command::Single(cmd) => get_filename_without_extension(cmd.as_str()),
    };

    filename = Path::new(&filename).file_name().unwrap_or_default().to_string_lossy().into_owned();

    if let Some(name) = the_name {
        filename.clone_from(&name);
        if is_cwl_file(&filename) {
            filename = filename.replace(".cwl", "");
        }
    }

    let foldername = filename.clone();
    filename.push_str(".cwl");

    format!("{}{foldername}/{filename}", get_workflows_folder())
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

    pathdiff::diff_paths(path, base_dir)
        .expect("path diffs not valid")
        .to_string_lossy()
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    pub fn os_path(path: &str) -> String {
        if cfg!(target_os = "windows") {
            Path::new(path).to_string_lossy().replace('/', "\\")
        } else {
            path.to_string()
        }
    }

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
    #[case(Command::Multiple(vec!["python".to_string(), "test/data/script.py".to_string()]), "workflows/script/script.cwl")]
    #[case(Command::Single("echo".to_string()), "workflows/echo/echo.cwl")]
    fn test_get_qualified_filename(#[case] command: Command, #[case] expected: &str) {
        assert_eq!(get_qualified_filename(&command, None), expected);
    }

    #[test]
    fn test_get_qualified_filename_with_name() {
        assert_eq!(
            get_qualified_filename(&Command::Single("echo".to_string()), Some("hello".to_string())),
            "workflows/hello/hello.cwl"
        );
    }
}
