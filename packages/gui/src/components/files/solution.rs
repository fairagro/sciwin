use crate::components::files::{Node, get_route};
use crate::components::{ICON_SIZE, SmallRoundActionButton};
use crate::files::{get_cwl_files, get_submodules_cwl_files};
use crate::layout::{INPUT_TEXT_CLASSES, RELOAD_TRIGGER, Route};
use crate::use_app_state;
use crate::reana_integration::{execute_reana_workflow, store_reana_credentials};
use dioxus::prelude::*;
use dioxus_free_icons::Icon;
use dioxus_free_icons::icons::go_icons::{GoCloud, GoFileDirectory, GoPlusCircle, GoTrash, GoGear, GoPlay};
use repository::Repository;
use repository::submodule::{add_submodule, remove_submodule};
use reqwest::Url;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use commonwl::execution::execute_cwlfile;
use crate::components::files::{FileType, read_node_type};

#[component]
pub fn SolutionView(project_path: ReadSignal<PathBuf>, dialog_signals: (Signal<bool>, Signal<bool>)) -> Element {
    let mut app_state = use_app_state();

    let files = use_memo(move || {
        RELOAD_TRIGGER(); //subscribe to changes
        get_cwl_files(project_path().join("workflows"))
    });
    let submodule_files = use_memo(move || {
        RELOAD_TRIGGER(); //subscribe to changes
        get_submodules_cwl_files(project_path())
    });

    let mut hover = use_signal(|| false);
    let mut adding = use_signal(|| false);
    let mut processing = use_signal(|| false);
    let mut new_package = use_signal(String::new);
    let mut show_settings = use_signal(|| false);
    let mut reana_instance = use_signal(String::new);
    let mut reana_token = use_signal(String::new);

    rsx! {
        div { class: "flex flex-grow flex-col overflow-y-auto",
            h2 { class: "mt-2 font-bold flex gap-1 items-center",
                Icon {
                    width: ICON_SIZE,
                    height: ICON_SIZE,
                    icon: GoFileDirectory,
                }
                if let Some(config) = &app_state.read().config {
                    "{config.workflow.name}"
                }
            }
            ul {
                onmouseenter: move |_| hover.set(true),
                onmouseleave: move |_| hover.set(false),
                for item in files() {
                    li {
                        class: "select-none",
                        draggable: true,
                        ondragstart: move |e| {
                            e.data_transfer().set_effect_allowed("all");
                            e.data_transfer().set_drop_effect("move");
                            app_state.write().set_data_transfer(&item)?;
                            e.data_transfer()
                                .set_data("application/x-allow-dnd", "1")
                                .map_err(|e| anyhow::anyhow!("{e}"))?;
                            Ok(())
                        },
                        div { class: "flex",
                            Link {
                                draggable: "false",
                                to: get_route(&item),
                                active_class: "font-bold",
                                class: "cursor-pointer select-none",
                                div { class: "flex gap-1 items-center",
                                    div {
                                        class: "flex",
                                        style: "width: {ICON_SIZE.unwrap()}px; height: {ICON_SIZE.unwrap()}px;",
                                        img { src: asset!("/assets/CWL.svg") }
                                    }
                                    "{item.name}"
                                }
                            }
                            if hover() {
                                SmallRoundActionButton {
                                    class: "hover:bg-fairagro-red-light",
                                    title: "Delete {item.name}",
                                    onclick: {
                                        //we need to double clone here ... ugly :/
                                        let item = item.clone();
                                        move |_| {
                                            let item = item.clone();
                                            async move {
                                                //0 open, 1 confirmed
                                                dialog_signals.0.set(true);
                                                loop {
                                                    if !dialog_signals.0() {
                                                        if dialog_signals.1() {
                                                            fs::remove_file(&item.path)?;
                                                            *RELOAD_TRIGGER.write() += 1;
                                                            let current_path = match use_route() {
                                                                Route::WorkflowView { path } => path.to_string(),
                                                                Route::ToolView { path } => path.to_string(),
                                                                _ => String::new(),
                                                            };
                                                            if current_path == item.path.to_string_lossy() {
                                                                router().push("/");
                                                            }
                                                        }
                                                        break;
                                                    }
                                                    tokio::time::sleep(Duration::from_millis(100)).await;
                                                }
                                                Ok(())
                                            }
                                        }
                                    },
                                    Icon {
                                        width: 10,
                                        height: 10,
                                        icon: GoTrash,
                                    }
                                }
                                // local
                                SmallRoundActionButton {
                                    class: "hover:bg-fairagro-mid-500",
                                    title: "Run locally",
                                    onclick: {
                                        let item = item.clone();
                                        let app_state = app_state;
                                        move |_| {
                                            let item = item.clone();
                                            let app_state = app_state;
                                            async move {
                                                let Some(dir) = app_state().working_directory.clone() else {
                                                    eprintln!("❌ No working directory");
                                                    return Ok(());
                                                };
                                                let args = vec![dir.join("inputs.yml").to_string_lossy().to_string()];
                                                tokio::task::spawn_blocking(move || {
                                                    let _ = execute_cwlfile(&item.path, &args, Some(dir));
                                                });
                                                Ok(())
                                            }
                                        }
                                    },
                                    Icon { width: 10, height: 10, icon: GoPlay }
                                }
                                // REANA 
                                if read_node_type(&item.path) == FileType::Workflow {
                                    SmallRoundActionButton {
                                        class: "hover:bg-fairagro-mid-500",
                                        title: format!("Execute with REANA"),
                                        onclick: {
                                            let item = item.clone();
                                            let show_settings = show_settings;
                                            let app_state = app_state;

                                            move |_| {
                                                let item = item.clone();
                                                let show_settings = show_settings;
                                                let working_dir = match app_state().working_directory.clone() {
                                                    Some(dir) => dir,
                                                    None => {
                                                        eprintln!("❌ No working directory set");
                                                        return Ok(());
                                                    }
                                                };
                                                spawn(async move {
                                                    execute_reana_workflow(item, working_dir, show_settings).await;
                                                });
                                            Ok(())
                                        }
                                    },
                                Icon {
                                        width: 10,
                                        height: 10,
                                        icon: GoCloud,
                                    }
                                }
                            }
                        }
                    }
                }
            }

            for (module , files) in submodule_files() {
                Submodule_View { module, files, dialog_signals }
            }
        }

            h2 {
                class: "mt-2 font-bold flex gap-1 items-center cursor-pointer",
                onclick: move |_| adding.set(true),
                Icon { width: ICON_SIZE, height: ICON_SIZE, icon: GoPlusCircle }
                if !adding() {
                    "Add package"
                } else if !processing() {
                    input {
                        class: "{INPUT_TEXT_CLASSES}",
                        r#type: "text",
                        value: "{new_package}",
                        placeholder: "package name",
                        oninput: move |e| new_package.set(e.value()),
                        onkeydown: move |e| {
                            if e.key() == Key::Enter {
                                e.prevent_default();
                                e.stop_propagation();                                
                                adding.set(false);
                                processing.set(true);
                                *RELOAD_TRIGGER.write() += 1;

                                let working_dir = app_state().working_directory.unwrap();
                                let mut repo = Repository::open(&working_dir)?;
                                let package = new_package();
                                let url = package.strip_suffix(".git").unwrap_or(&package);
                                
                                let url_obj = Url::parse(url)?;
                                
                                let package_dir = Path::new("packages");
                                let repo_name = url_obj.path().strip_prefix("/").unwrap();
                                add_submodule(&mut repo, url, &None, &working_dir.join(package_dir.join(repo_name)))?;
                                
                                *RELOAD_TRIGGER.write() += 1;
                                processing.set(false);
                            }
                            Ok(())
                        },
                    }
                }
                else {
                    "..."
                }
            }
            //REANA SETTINGS
            div { class: "border rounded p-3",
                h2 {
                    class: "font-bold flex items-center gap-2 cursor-pointer",
                    onclick: move |_| show_settings.set(!show_settings()),
                    Icon { width: ICON_SIZE, height: ICON_SIZE, icon: GoGear },
                    "REANA Settings"
                }

                if show_settings() {
                    div { class: "flex flex-col gap-2 mt-2",
                        input {
                            class: "{INPUT_TEXT_CLASSES}",
                            placeholder: "REANA instance URL",
                            oninput: move |e| reana_instance.set(e.value()),
                        }
                        input {
                            class: "{INPUT_TEXT_CLASSES}",
                            r#type: "password",
                            placeholder: "REANA access token",
                            oninput: move |e| reana_token.set(e.value()),
                        }

                        div { class: "flex justify-end gap-2",
                            button {
                                class: "px-3 py-1 rounded bg-zinc-200",
                                onclick: move |_| show_settings.set(false),
                                "Cancel"
                            }
                            button {
                                class: "px-3 py-1 rounded bg-fairagro-mid-500 text-white",
                                onclick: move |_| {
                                    let i = reana_instance();
                                    let t = reana_token();
                                    if i.is_empty() || t.is_empty() {
                                        eprintln!("❌ Instance or token empty");
                                        return;
                                    }
                                    if let Err(e) = store_reana_credentials(&i, &t) {
                                        eprintln!("❌ Failed to store credentials: {e}");
                                        return;
                                    }
                                    show_settings.set(false);
                                },
                                "Save"
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
pub fn Submodule_View(module: String, files: Vec<Node>, dialog_signals: (Signal<bool>, Signal<bool>)) -> Element {
    let mut app_state = use_app_state();
    let mut hover = use_signal(|| false);

    rsx! {
        div {
            onmouseenter: move |_| hover.set(true),
            onmouseleave: move |_| hover.set(false),
            h2 { class: "mt-2 font-bold flex gap-1 items-center h-4",
                Icon { width: ICON_SIZE, height: ICON_SIZE, icon: GoCloud }
                "{module}"
                SmallRoundActionButton {
                    class: "ml-auto mr-3 hover:bg-fairagro-red-light",
                    title: "Uninstall {module}",
                    onclick: move |_| {
                        let module = module.clone();
                        async move {
                            //0 open, 1 confirmed
                            dialog_signals.0.set(true);
                            loop {
                                if !dialog_signals.0() {
                                    if dialog_signals.1() {
                                        let repo = Repository::open(
                                            //reset

                                            app_state().working_directory.unwrap(),
                                        )?;
                                        remove_submodule(&repo, &module)?;
                                        *RELOAD_TRIGGER.write() += 1;
                                        dialog_signals.1.set(false);
                                    }
                                    break;
                                }
                                tokio::time::sleep(Duration::from_millis(100)).await;
                            }
                            Ok(())
                        }
                    },
                    if hover() {
                        Icon {
                            width: ICON_SIZE,
                            height: ICON_SIZE,
                            icon: GoTrash,
                        }
                    }
                }
            }
            ul {
                for item in files {
                    li {
                        class: "select-none",
                        draggable: true,
                        ondragstart: move |e| {
                            e.data_transfer().set_effect_allowed("all");
                            e.data_transfer().set_drop_effect("move");
                            app_state.write().set_data_transfer(&item)?;
                            e.data_transfer()
                                .set_data("application/x-allow-dnd", "1")
                                .map_err(|e| anyhow::anyhow!("{e}"))?;
                            Ok(())
                        },
                        Link {
                            draggable: "false",
                            to: get_route(&item),
                            active_class: "font-bold",
                            class: "cursor-pointer select-none",
                            div { class: "flex gap-1 items-center",
                                div {
                                    class: "flex",
                                    style: "width: {ICON_SIZE.unwrap()}px; height: {ICON_SIZE.unwrap()}px;",
                                    img { src: asset!("/assets/CWL.svg") }
                                }

                                "{item.name}"
                            }
                        }
                    }
                }
            }
        }
    }
}