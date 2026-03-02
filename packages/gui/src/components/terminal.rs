use crate::{
    components::{ICON_SIZE, SmallRoundActionButton, ToastItem},
    use_app_state,
};
use dioxus::prelude::*;
use dioxus_free_icons::{
    Icon,
    icons::go_icons::{GoDownload, GoCloud, GoPackage},
};
use serde::{Deserialize, Serialize};
use crate::reana_integration::{get_reana_credentials, store_reana_credentials, delete_reana_credentials};
use crate::reana_integration::get_last_workflow_name;
use remote_execution::export_rocrate;


#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum ExecutionType {
    #[default]
    Remote,
    Local,
}

#[component]
pub fn TerminalViewer(
    #[props(optional)] exec_type: Option<ExecutionType>,
) -> Element {
    let exec_type = exec_type.unwrap_or(ExecutionType::Local);
    let app_state = use_app_state();
    let terminal_signal = &app_state().terminal_log;
    let show_terminal_log = app_state().show_terminal_log;
    let mut toast_items = use_context::<Signal<Vec<ToastItem>>>();
    let show_modal = &app_state().show_manage_reana_modal;

    let title = match exec_type {
        ExecutionType::Local => "Local Execution",
        ExecutionType::Remote => "Remote Execution",
    };

    rsx! {
        div {
            id: "terminal",
            class: "relative flex flex-col w-full h-full min-h-0 overflow-hidden border border-gray-300 rounded-lg",
            div {
                class: "flex justify-between items-center bg-gray-100 px-3 py-2 border-b border-gray-300",
                h2 { class: "text-sm font-semibold text-gray-700", "{title}" }
                if exec_type == ExecutionType::Remote {
                    div {
                         class: "flex items-center space-x-1 px-2 py-1 rounded-md select-none",
                        // Download Results button
                        SmallRoundActionButton {
                            onclick: move |_| {
                                toast_items.write().push(ToastItem::new(
                                    "Download Results".to_string(),
                                    "Downloading files...".to_string(),
                                    3,
                                ));
                                let output_dir: Option<String> = app_state()
                                    .working_directory
                                    .as_ref()
                                    .map(|path| path.to_string_lossy().to_string());
                                use_coroutine(move |mut _co: dioxus::prelude::UnboundedReceiver<()>| {
                                    let output_dir = output_dir.clone();
                                    async move {
                                        let workflow_name: String = match get_last_workflow_name().await {
                                            Ok(name) => name,
                                            Err(e) => {
                                                eprintln!("❌ Failed to get workflow name: {e}");
                                                return;
                                            }
                                        };
                                        let result = tokio::task::spawn_blocking(move || {
                                            remote_execution::download_results(
                                                &workflow_name,
                                                false,
                                                output_dir.as_ref(),
                                            )
                                            .map_err(|e| e.to_string())
                                        }).await;
                                        match result {
                                            Ok(Ok(())) => eprintln!("✅ Download completed successfully."),
                                            Ok(Err(e)) => eprintln!("❌ Download failed: {e}"),
                                            Err(e) => eprintln!("❌ Task panicked: {e}"),
                                        }
                                    }
                                });
                            },
                            div {
                                class: "flex items-center space-x-1",
                                Icon { icon: GoDownload, width: ICON_SIZE, height: ICON_SIZE }
                                span { "Download Results" }
                            }
                        }
                        // Export RO-Crate button
                        SmallRoundActionButton {
                            onclick: move |_| {
                                toast_items.write().push(ToastItem::new(
                                    "Export RO-Crate".to_string(),
                                    "Exporting RO-Crate...".to_string(),
                                    3,
                                ));
                                let working_dir: Option<String> = app_state()
                                    .working_directory
                                    .as_ref()
                                    .map(|path| path.to_string_lossy().to_string());
                                let output_dir: Option<String> = working_dir.clone().map(|dir| {
                                    std::path::Path::new(&dir)
                                        .join("rocrate")
                                        .to_string_lossy()
                                        .into_owned()
                                });
                                use_coroutine(move |mut _co: dioxus::prelude::UnboundedReceiver<()>| {
                                    let working_dir = working_dir.clone();
                                    let output_dir = output_dir.clone();
                                    async move {
                                        let workflow_name: String = match get_last_workflow_name().await {
                                            Ok(name) => name,
                                            Err(e) => {
                                                eprintln!("❌ Failed to get workflow name: {e}");
                                                return;
                                            }
                                        };
                                        let result = tokio::task::spawn_blocking(move || {
                                            export_rocrate(
                                                &workflow_name,
                                                output_dir.as_ref(),
                                                working_dir.as_ref(),
                                            )
                                            .map_err(|e| e.to_string())
                                        }).await;
                                        match result {
                                            Ok(Ok(())) => eprintln!("✅ RO-Crate exported successfully."),
                                            Ok(Err(e)) => eprintln!("❌ RO-Crate export failed: {e}"),
                                            Err(e) => eprintln!("❌ Task panicked: {e}"),
                                        }
                                    }
                                });
                            },
                            div {
                                class: "flex items-center space-x-1",
                                Icon { icon: GoPackage, width: ICON_SIZE, height: ICON_SIZE }
                                span { "Export RO-Crate" }
                            }
                        }
                        ManageReanaButton {
                                show_modal: *show_modal
                            }
                    }
                }
            }
            if show_terminal_log() {
                div {
                    id: "editor-container",
                    class: "flex-1 overflow-y-scroll bg-black text-green-400 text-xs font-mono p-2 whitespace-pre-wrap",
                    pre { "{terminal_signal()}" }
                }
            }
        }
    }
}

#[allow(dead_code)]
struct ReanaInstance {
    url: String,
    token: String,
}

#[component]
pub fn ManageReanaButton(
    #[props(optional)] show_modal: Option<Signal<bool>>,
) -> Element {
    let mut show_modal = show_modal.unwrap_or(use_signal(|| false));
    let mut instances = use_signal(Vec::<ReanaInstance>::new);
    let mut new_url = use_signal(String::new);
    let mut new_token = use_signal(String::new);
    let mut toast_items = use_context::<Signal<Vec<ToastItem>>>();
    {
        use_effect(move || {
            match get_reana_credentials() {
                Ok(Some((url, token))) => {
                    instances.write().push(ReanaInstance { url, token });
                }
                Ok(None) | Err(_) => {
                    show_modal.set(true);
                }
            }
        });
    }

    let mut push_toast = move |title: &str, message: String, duration_secs: i64| {
        toast_items.write().push(ToastItem::new(title.to_string(), message, duration_secs));
    };

    let instance_list = {
        let list = instances.read();
        if list.is_empty() {
            rsx! { p { class: "text-gray-500", "No instances configured." } }
        } else {
            rsx! {
                ul {
                    class: "space-y-2",
                    {list.iter().enumerate().map(|(i, instance)| {
                        rsx! {
                            li {
                                key: "{i}",
                                class: "flex justify-between items-center border p-2 rounded",
                                div { span { class: "font-medium", "{instance.url}" } }
                                button {
                                    class: "text-red-600 hover:text-red-800",
                                    onclick: move |_| {
                                        if let Err(e) = delete_reana_credentials() {
                                            push_toast("Error", format!("Failed to delete credentials: {e}"), 3);
                                        } else {
                                            push_toast("Deleted", "REANA credentials removed successfully".to_string(), 2);
                                            instances.write().remove(i);
                                        }
                                    },
                                    "Delete"
                                }
                            }
                        }
                    }) }
                }
            }
        }
    };

    rsx! {
        SmallRoundActionButton {
            onclick: move |_| {
                push_toast("Manage REANA Instances", "Opening REANA manager...".to_string(), 3);
                show_modal.set(true);
            },
            div {
                class: "flex items-center space-x-1",
                Icon { icon: GoCloud, width: ICON_SIZE, height: ICON_SIZE }
                span { "Manage REANA Instances" }
            }
        }
        if show_modal() {
            div {
                class: "fixed top-20 right-20 bg-white p-6 rounded-lg shadow-lg w-96 space-y-4 z-50",
                h2 { class: "text-lg font-bold mb-2", "Configured REANA Instances" }
                {instance_list}

                div {
                    class: "space-y-2 border-t pt-4",
                    h3 { class: "font-semibold", "Add New Instance" }
                    input {
                        class: "w-full border rounded px-2 py-1",
                        placeholder: "Instance URL",
                        value: "{new_url()}",
                        oninput: move |e| new_url.set(e.value()),
                    }
                    input {
                        class: "w-full border rounded px-2 py-1",
                        placeholder: "API Token",
                        value: "{new_token()}",
                        oninput: move |e| new_token.set(e.value()),
                    }
                    div {
                        class: "flex justify-end space-x-2 mt-2",
                        button {
                            class: "bg-blue-600 text-white px-3 py-1 rounded hover:bg-blue-700",
                            onclick: move |_| {
                                if !new_url().is_empty() && !new_token().is_empty() {
                                    if let Err(err) = store_reana_credentials(&new_url(), &new_token()) {
                                        push_toast("Error", format!("Failed to store credentials: {}", err), 4);
                                    } else {
                                        push_toast("Success", "REANA credentials saved successfully".to_string(), 2);
                                        instances.write().push(ReanaInstance { url: new_url(), token: new_token() });
                                        new_url.set(String::new());
                                        new_token.set(String::new());
                                    }
                                }
                            },
                            "Add Instance"
                        }
                        button {
                            class: "bg-gray-400 text-white px-3 py-1 rounded hover:bg-gray-500",
                            onclick: move |_| show_modal.set(false),
                            "Close"
                        }
                    }
                }
            }
        }
    }
}