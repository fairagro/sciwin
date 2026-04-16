use crate::layout::{INPUT_TEXT_CLASSES, RELOAD_TRIGGER, Route};
use crate::{components::Terminal, use_app_state};
use commonwl::CommandLineTool;
use commonwl::execution::ContainerEngine;
use dioxus::prelude::*;
use dioxus_free_icons::icons::go_icons::GoSync;
use dioxus_free_icons::{Icon, icons::go_icons::GoAlert};
use repository::Repository;
use s4n_core::io;
use s4n_core::tool::{ContainerInfo, ToolCreationOptions, create_tool};
use std::env;
use std::time::Duration;

#[component]
pub fn ToolAddForm() -> Element {
    let app_state = use_app_state();
    let working_dir = use_memo(move || app_state.read().working_directory.clone());
    let mut modified_files = use_signal(Vec::new);

    let mut container_image = use_signal(|| None::<String>);
    let mut container_tag = use_signal(|| None::<String>);
    let mut name = use_signal(|| None::<String>);
    let command = use_signal(String::new);
    let mut enable_network = use_signal(|| false);

    let mut running = use_signal(|| false);

    use_coroutine(move |_rx: UnboundedReceiver<()>| async move {
        loop {
            let working_dir = {
                let st = app_state.read();
                st.working_directory.clone()
            };

            if let Some(working_dir) = working_dir
                && let Ok(repo) = Repository::open(working_dir)
            {
                modified_files.set(repository::get_modified_files(&repo));
            }

            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    });

    rsx!(
        form {
            class: "mx-5 py-3 min-h-full text-sm flex flex-col gap-4 bg-zinc-100 px-4 border-1 border-zinc-400",
            onsubmit: move |_| async move {
                //prevent little mistakes
                if running() {
                    return Ok(());
                }
                running.set(true);

                //could refactor the create tool method to not use current dir...
                let current = env::current_dir()?;
                env::set_current_dir(working_dir().unwrap())?;
                let tool_name = name();
                let container_image = container_image();
                let container_tag = container_tag();
                let enable_network = enable_network();
                let command = command();

                let result = tokio::task::spawn_blocking(move || {
                    let container_options = container_image
                        .as_ref()
                        .map(|image| ContainerInfo {
                            image,
                            tag: container_tag.as_deref(),
                        });
                    let command = shlex::split(&command).unwrap();
                    let run_container = if container_options.is_some() {
                        ContainerEngine::auto()
                    } else {

                        None
                    };
                    let options = ToolCreationOptions {
                        command: &command,
                        container: container_options,
                        enable_network,
                        commit: true,
                        run_container,
                        ..Default::default()
                    };
                    create_tool(&options, tool_name, true)
                });

                let yaml = result.await??;
                let cwl: CommandLineTool = serde_yaml::from_str(&yaml)?;
                let path = io::get_qualified_filename(&cwl.base_command, name());
                let path = working_dir().unwrap().join(path);
                *RELOAD_TRIGGER.write() += 1;
                env::set_current_dir(current)?;
                running.set(false);
                navigator()
                    .push(Route::ToolView {
                        path: path.to_string_lossy().to_string(),
                    });
                Ok(())
            },
            h2 { class: "text-lg text-fairagro-dark-500 font-bold", "New CommandLineTool" }
            if !modified_files().is_empty() {
                div { class: "bg-fairagro-red-light border-fairagro-red border-2 px-3 py-2 flex gap-2 items-center",
                    div { class: "text-red-900",
                        Icon { width: 24, height: 24, icon: GoAlert }
                    }
                    p {
                        "Your project is not in a clean state, this leads to wrong results! Please commit before creating a new CommandLineTool!"
                    }
                }
            }
            div { class: "flex flex-col gap-1",
                label { r#for: "name",
                    "Name"
                    span { class: "ml-2 bg-fairagro-dark-500 px-1 py-0.5 rounded-md text-xs text-zinc-100 ring-fairagro-dark-200/20",
                        "optional"
                    }
                }
                input {
                    class: "{INPUT_TEXT_CLASSES} w-70",
                    r#type: "text",
                    oninput: move |e| {
                        if !e.value().is_empty() {
                            name.set(Some(e.value()));
                        } else {
                            name.set(None);
                        }
                    },
                }
            }
            div { class: "flex flex-col gap-1",
                label { r#for: "command", "Command" }
                Terminal { value: command }
                span { class: "text-xs text-zinc-500",
                    "The command the CommandLineTool shall resemble. Tab and Arrow keys can be used to use completion system."
                }
            }

            div { class: "flex",
                div { class: "flex flex-col gap-1",
                    label { r#for: "container",
                        "Container"
                        span { class: "ml-2 bg-fairagro-dark-500 px-1 py-0.5 rounded-md text-xs text-zinc-100 ring-fairagro-dark-200/20",
                            "optional"
                        }
                    }
                    input {
                        class: "{INPUT_TEXT_CLASSES} w-70",
                        r#type: "text",
                        oninput: move |e| {
                            if !e.value().is_empty() {
                                container_image.set(Some(e.value()));
                            } else {
                                container_image.set(None);
                            }

                        },
                    }
                    span { class: "text-xs text-zinc-500",
                        "Name of the Image to be pulled from a registry (Dockerhub) e.g. python:3.12 or just 'Dockerfile' for a local Dockerfile"
                    }
                }

                div { class: "flex flex-col gap-1",
                    label { r#for: "container",
                        "Container Image Tag"
                        span { class: "ml-2 bg-fairagro-dark-500 px-1 py-0.5 rounded-md text-xs text-zinc-100 ring-fairagro-dark-200/20",
                            "optional"
                        }
                    }
                    input {
                        class: "{INPUT_TEXT_CLASSES} w-70",
                        r#type: "text",
                        oninput: move |e| {
                            if !e.value().is_empty() {
                                container_tag.set(Some(e.value()));
                            } else {
                                container_tag.set(None);
                            }

                        },
                    }
                    span { class: "text-xs text-zinc-500",
                        "Name which is used to tag container after built. Mandatory when Dockerfile is used."
                    }
                }
            }

            div {
                div { class: "flex gap-2 items-center",
                    input {
                        id: "net",
                        r#type: "checkbox",
                        oninput: move |e| { enable_network.set(e.checked()) },
                    }
                    label { r#for: "net", "Enable network connection" }
                }
                span { class: "text-xs text-zinc-500",
                    "The tool needs access to the internet (neccessary if container is used)"
                }
            }
            if !running() {
                input {
                    class: "text-white bg-fairagro-mid-500 hover:bg-fairagro-dark-500 mx-auto px-3 py-2 rounded-md",
                    r#type: "submit",
                    value: "Run & Create",
                    disabled: running(),
                }
            } else {
                div {
                    class: "mx-auto text-fairagro-dark-500 animate-spin",
                    title: "Command is running",
                    Icon { width: 48, height: 48, icon: GoSync }
                }
            }
        }
    )
}
