pub mod cli;
pub mod commands;
pub mod cwl;
pub mod interactive;
pub mod logger;

use colored::Colorize;
use tracing::info;
use similar::{ChangeTag, TextDiff};
use std::fmt;

#[derive(Debug)]
pub struct ExitCode(pub i32);

impl std::fmt::Display for ExitCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "exit code {}", self.0)
    }
}

impl std::error::Error for ExitCode {}

/// Pretty-prints `value` as JSON to stdout, the only output of a `--json` command.
#[allow(clippy::disallowed_macros)]
pub fn print_json(value: &impl serde::Serialize) -> miette::Result<()> {
    use miette::IntoDiagnostic;
    println!("{}", serde_json::to_string_pretty(value).into_diagnostic()?);
    Ok(())
}

pub fn print_list(list: &Vec<String>) {
    for item in list {
        info!("\t- {item}");
    }
}

struct Line(Option<usize>);
impl fmt::Display for Line {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some(n) => write!(f, "{:>4}", n + 1),
            None => write!(f, "    "),
        }
    }
}

pub fn print_diff(old: &str, new: &str) {
    let diff = TextDiff::from_lines(old, new);
    for (idx, group) in diff.grouped_ops(3).iter().enumerate() {
        if idx > 0 {
            eprintln!("{:-^1$}", "-", 80); //print line to separate groups
        }

        for op in group {
            for change in diff.iter_changes(op) {
                let sign = match change.tag() {
                    ChangeTag::Delete => "-",
                    ChangeTag::Insert => "+",
                    ChangeTag::Equal => " ",
                };

                let (old_line, new_line) = (Line(change.old_index()), Line(change.new_index()));

                let styled_line = match change.tag() {
                    ChangeTag::Equal => format!("{sign} {}", change.value()).dimmed(),
                    ChangeTag::Delete => format!("{sign} {}", change.value()).red(),
                    ChangeTag::Insert => format!("{sign} {}", change.value()).green(),
                };

                eprint!("{old_line} {new_line} | {styled_line}");
            }
        }
    }
}
