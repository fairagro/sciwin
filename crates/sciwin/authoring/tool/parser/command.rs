//! Recognizing which leading tokens of a command line form its base command.
//!
//! `authoring::paths` names a tool after whatever token this module identified as the
//! payload, so the two must agree -- see [`matches_script_executor`].

use commonwl::OneOrMany;

//TODO complete list
pub static SCRIPT_EXECUTORS: &[&str] = &["python", "python3", "R", "Rscript", "node", "java"];
pub static SCRIPT_MODIFIERS: &[&str] = &["-e", "-m"];

/// Whether a token names a script interpreter, allowing a version suffix (`python3.11`).
///
/// The character after the matched prefix must be non-alphabetic, so `Rake` doesn't match
/// `R` and `pythonic` doesn't match `python`.
pub(crate) fn matches_script_executor(token: &str) -> bool {
    SCRIPT_EXECUTORS.iter().any(|&exec| {
        token == exec
            || (token.starts_with(exec)
                && token[exec.len()..]
                    .chars()
                    .next()
                    .is_some_and(|c| !c.is_alphabetic()))
    })
}

/// Whether a token is an interpreter modifier that shifts the payload one position right,
/// as in `python -m module` or `R -e script.R`.
pub(crate) fn matches_script_modifier(token: &str) -> bool {
    SCRIPT_MODIFIERS
        .iter()
        .any(|&modif| token.starts_with(modif))
}

/// Splits the leading tokens that form the command itself off from its arguments.
///
/// `echo hello` has a one-token base command; `python script.py` and `python -m module`
/// have two and three, because the script is part of what's being run, not an argument.
pub(crate) fn get_base_command(command: &[&str]) -> OneOrMany<String> {
    if command.is_empty() {
        return OneOrMany::One(String::new());
    }

    let mut base_command = vec![command[0].to_string()];

    if command.len() > 1 && matches_script_executor(command[0]) {
        if command.len() > 2 && matches_script_modifier(command[1]) {
            base_command.push(command[1].to_string()); //the modifier
            base_command.push(command[2].to_string()); //the package
        } else {
            base_command.push(command[1].to_string());
        }
    }

    match base_command.len() {
        1 => OneOrMany::One(command[0].to_string()),
        _ => OneOrMany::Many(base_command),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case("python script.py --arg1 hello", OneOrMany::Many(vec!["python".to_string(), "script.py".to_string()]))]
    #[case("echo 'Hello World!'", OneOrMany::One("echo".to_string()))]
    #[case("Rscript lol.R", OneOrMany::Many(vec!["Rscript".to_string(), "lol.R".to_string()]))]
    #[case("", OneOrMany::One(String::new()))]
    #[case("python", OneOrMany::One("python".to_string()))]
    #[case("Rake build", OneOrMany::One("Rake".to_string()))]
    #[case("python3.11 script.py", OneOrMany::Many(vec!["python3.11".to_string(), "script.py".to_string()]))]
    #[case("python3 -m my_module", OneOrMany::Many(vec!["python3".to_string(), "-m".to_string(), "my_module".to_string()]))]
    pub fn test_get_base_command(#[case] command: &str, #[case] expected: OneOrMany<String>) {
        let args = shlex::split(command).unwrap();
        let args_slice: Vec<&str> = args.iter().map(AsRef::as_ref).collect();

        let result = get_base_command(&args_slice);
        assert_eq!(result, expected);
    }
}
