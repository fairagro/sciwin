//! The shell syntax a command line can carry: redirections and pipes.

use commonwl::{
    IntegerOrExpression,
    documents::Argument,
    inputs::{CommandInputParameter, CommandLineBinding},
};

/// Tokens that must reach the shell unquoted to keep their meaning.
static SPECIAL_CHARS: &[&str] = &["|", ">", "2>"];

/// Reads the target of a redirection out of `remaining_args`, which starts at the redirect
/// token itself. `None` for a dangling redirect with nothing after it.
pub(super) fn handle_redirection(remaining_args: &[&str]) -> Option<String> {
    //hopefully? most cases are only `some_command > some_file.out`
    //redirect token comes at pos 0, discard that; pos 1 is the filename, if present
    //(absent e.g. for a trailing dangling `>` with nothing after it)
    remaining_args.get(1).map(|out_file| out_file.to_string())
}

/// Replays everything after a `|` as literal CWL arguments, positioned after the inputs.
///
/// Note this bakes the piped stage in verbatim -- nothing in it becomes a tool input.
pub(super) fn collect_arguments(
    piped: &[&str],
    inputs: &[CommandInputParameter],
) -> Option<Vec<Argument>> {
    if piped.is_empty() {
        return None;
    }

    let piped_args = piped.iter().enumerate().map(|(i, &x)| {
        let shell_quote = if SPECIAL_CHARS.contains(&x) {
            Some(false)
        } else {
            None
        };
        Argument::Binding(CommandLineBinding {
            position: Some(IntegerOrExpression::Int(
                (inputs.len() + i).try_into().unwrap_or_default(),
            )),
            value_from: Some(x.to_string()),
            shell_quote,
            ..Default::default()
        })
    });

    let mut args = vec![Argument::Binding(CommandLineBinding {
        position: Some(IntegerOrExpression::Int(
            inputs.len().try_into().unwrap_or_default(),
        )),
        value_from: Some("|".to_string()),
        shell_quote: Some(false),
        ..Default::default()
    })];
    args.extend(piped_args);

    Some(args)
}

/// Splits `slice` at the first occurrence of `split_at`, dropping the separator. The whole
/// slice ends up on the left when the separator isn't present.
pub(super) fn split_at_first<'a>(
    slice: &'a [&'a str],
    split_at: &str,
) -> (&'a [&'a str], &'a [&'a str]) {
    match slice.iter().position(|x| *x == split_at) {
        Some(index) => (&slice[..index], &slice[index + 1..]),
        None => (slice, &[]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_at_first() {
        let tokens = ["df", "|", "grep", "tmpfs"];
        let (lhs, rhs) = split_at_first(&tokens, "|");
        assert_eq!(lhs, ["df"]);
        assert_eq!(rhs, ["grep", "tmpfs"]);
    }

    #[test]
    fn test_split_at_first_absent_separator() {
        let tokens = ["df", "-h"];
        let (lhs, rhs) = split_at_first(&tokens, "|");
        assert_eq!(lhs, ["df", "-h"]);
        assert!(rhs.is_empty());
    }

    #[test]
    fn test_handle_redirection_dangling() {
        assert_eq!(handle_redirection(&[">"]), None);
        assert_eq!(
            handle_redirection(&[">", "out.txt"]),
            Some("out.txt".to_string())
        );
    }
}
