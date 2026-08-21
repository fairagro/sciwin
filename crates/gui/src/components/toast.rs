use crate::components::sleep;
use dioxus::prelude::*;
use dioxus_free_icons::{Icon, icons::go_icons::GoInfo};

#[derive(Clone, Debug)]
pub struct ToastItem {
    title: String,
    message: String,
    delete_at: i64,
}

impl ToastItem {
    pub fn new(title: String, message: String, duration: i64) -> Self {
        Self {
            title,
            message,
            delete_at: chrono::Local::now().timestamp() + duration,
        }
    }
}

#[component]
pub fn ToastProvider() -> Element {
    let mut items = use_context::<Signal<Vec<ToastItem>>>();

    use_resource(move || async move {
        loop {
            items.write().retain(|item| chrono::Local::now().timestamp() < item.delete_at);
            sleep(100).await;
        }
    });

    rsx! {
        div { class: "absolute right-5 bottom-8 z-10 select-none",
            //message as children
            for item in items() {
                div {
                    class: "my-2 shadow-sm bg-zinc-200 min-w-64 border-l-5 border-fairagro-dark-500 rounded-md flex items-center px-2",
                    role: "alert",
                    Icon {
                        icon: GoInfo,
                        width: 32,
                        height: 32,
                        class: "text-fairagro-mid-500",
                    }
                    div {
                        h4 { class: "px-4 pt-2 font-bold text-sm", {item.title} }
                        p { class: "px-4 py-1", {item.message} }
                    }
                }
            }
        }
    }
}
