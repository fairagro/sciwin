mod button;
mod code;
mod dialog;
pub mod files;
pub mod graph;
pub mod layout;
mod term;
mod toast;
mod tool_add_form;
mod terminal;

pub use button::*;
pub use code::*;
pub use dialog::*;
pub use term::*;
pub use toast::*;
pub use tool_add_form::*;
pub use terminal::*;

use dioxus::prelude::*;
use dioxus_free_icons::{Icon, icons::go_icons::GoRocket};

pub const ICON_SIZE: Option<u32> = Some(14);

#[component]
pub fn NoProject() -> Element {
    rsx! {
        div { class: "flex flex-col items-center mt-10 gap-4 text-lg text-center text-zinc-400",
            Icon { width: Some(64), height: Some(64), icon: GoRocket }
            div { "Start by loading up a project" }
        }
    }
}

pub(crate) async fn sleep(interval: usize) {
    tokio::time::sleep(tokio::time::Duration::from_millis(interval as u64)).await;
}
