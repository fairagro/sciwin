use dialoguer::{Input, theme::ColorfulTheme};
use log::warn;
use repository::Config;

mod annotate;
mod connect;
mod create;
mod execute;
mod init;
mod list;
mod packages;
mod remove;
mod save;
mod visualize;

pub use annotate::*;
pub use connect::*;
pub use create::*;
pub use execute::*;
pub use init::*;
pub use list::*;
pub use packages::*;
pub use remove::*;
pub use save::*;
pub use visualize::*;

pub fn check_git_config() -> anyhow::Result<()> {
    let mut config = Config::open_default()?;
    if config.get_string("user.name").is_err() || config.get_string("user.email").is_err() {
        warn!("User configuration not found!");

        let name: String = Input::with_theme(&ColorfulTheme::default())
            .with_prompt("Enter your name")
            .interact_text()?;
        config.set_str("user.name", name.trim())?;

        let mail: String = Input::with_theme(&ColorfulTheme::default())
            .with_prompt("Enter your email")
            .interact_text()?;
        config.set_str("user.email", mail.trim())?;
    }
    Ok(())
}
