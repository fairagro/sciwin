//! Decides whether the CLI may ask the user something.
use std::io::IsTerminal;
use std::sync::atomic::{AtomicBool, Ordering};

static ASSUME_YES: AtomicBool = AtomicBool::new(false);

/// Set from the global `--yes` flag.
pub fn set_assume_yes(yes: bool) {
    ASSUME_YES.store(yes, Ordering::Relaxed);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Prompt {
    /// `--yes` given: confirm everything and take the default choice.
    AssumeYes,
    /// A user is attached, ask them.
    Ask,
    /// Nobody can answer (agent, script, CI). Fail with a hint instead of hanging.
    Unavailable,
}

/// dialoguer reads answers from stdin and draws on stderr, so both must be terminals.
pub fn prompt_mode() -> Prompt {
    if ASSUME_YES.load(Ordering::Relaxed) {
        Prompt::AssumeYes
    } else if std::io::stdin().is_terminal() && std::io::stderr().is_terminal() {
        Prompt::Ask
    } else {
        Prompt::Unavailable
    }
}
