use crate::{
    components::{ICON_SIZE, files::Node},
    files::{get_cwl_files, get_submodules_cwl_files},
    use_app_state,
};
use commonwl::{CWLType, load_doc};
use dioxus::{html::geometry::ClientPoint, prelude::*};
use dioxus_free_icons::{Icon, icons::go_icons::GoChevronRight};
use std::path::PathBuf;

#[component]
pub fn NodeAddForm(open: Signal<bool>, pos: Signal<ClientPoint>, project_path: ReadSignal<PathBuf>) -> Element {
    let app_state = use_app_state();
    let files = use_memo(move || {
        open();
        get_cwl_files(project_path().join("workflows"))
    });
    let submodule_files = use_memo(move || {
        open();
        get_submodules_cwl_files(project_path())
    });

    rsx! {
        if open() {
            div {
                class: "absolute z-15",
                style: "left: {pos().x}px; top: {pos().y}px;",
                onclick: move |_| open.set(false),
                ul {
                    li {
                        InputOutputMenu { is_input: true, top_level_menu: open }
                    }
                    li {
                        InputOutputMenu { is_input: false, top_level_menu: open }
                    }
                    li {
                        NodeAddItem {
                            //at this point there needs to be a config in place
                            name: app_state.read().config.clone().unwrap().workflow.name,
                            files: files(),
                        }
                    }
                    for (module , files) in submodule_files() {
                        li {
                            NodeAddItem { name: module, files }
                        }
                    }
                }
            }
        }
    }
}

#[component]
pub fn NodeAddItem(name: String, files: Vec<Node>) -> Element {
    let mut app_state = use_app_state();
    let mut open = use_signal(|| false);

    rsx! {
        div {
            class: "flex",
            onmouseenter: move |_| open.set(true),
            onmouseleave: move |_| open.set(false),
            div { class: "flex w-48 bg-fairagro-light-200/80 hover:bg-fairagro-light-400 px-2 py-1.25 items-center justify-end",
                "{name}"
                div { class: "ml-auto",
                    Icon {
                        width: ICON_SIZE,
                        height: ICON_SIZE,
                        icon: GoChevronRight,
                    }
                }
            }
            if open() {
                div { class: "ml-auto absolute left-48",
                    ul {
                        for file in files {
                            li { class: "min-w-48 px-2 py-1.25 items-center bg-fairagro-light-200/80 hover:bg-fairagro-light-400",
                                button {
                                    onclick: move |_| {
                                        let mut cwl = load_doc(&file.path).map_err(|e| anyhow::anyhow!("{e}"))?;
                                        let working_dir = app_state().working_directory.unwrap();
                                        if let Some(path_relative_to_root) = pathdiff::diff_paths(
                                            &file.path,
                                            &working_dir,
                                        ) {
                                            let name = file.name.strip_suffix(".cwl").unwrap_or(&file.name);
                                            app_state
                                                .write()
                                                .workflow
                                                .add_new_step_if_not_exists(
                                                    name,
                                                    path_relative_to_root.to_string_lossy().as_ref(),
                                                    &mut cwl,
                                                    &working_dir,
                                                )?;
                                        }
                                        Ok(())
                                    },
                                    "{file.name}"
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
pub fn InputOutputMenu(is_input: bool, top_level_menu: Signal<bool>) -> Element {
    let mut show_types = use_signal(|| false);
    let mut active_type_index = use_signal(|| None::<usize>);

    rsx! {
        div {
            class: "flex relative",
            onmouseenter: move |_| show_types.set(true),
            onmouseleave: move |_| {
                show_types.set(false);
                active_type_index.set(None);
            },

            div { class: "flex w-48 bg-fairagro-light-200/80 hover:bg-fairagro-light-400 px-2 py-1.25 items-center justify-end",
                if is_input {
                    "Input"
                } else {
                    "Output"
                }
                div { class: "ml-auto",
                    Icon {
                        width: ICON_SIZE,
                        height: ICON_SIZE,
                        icon: GoChevronRight,
                    }
                }
            }

            if show_types() {
                div { class: "absolute left-48 top-0",
                    ul {
                        for (idx , type_) in type_iter().enumerate() {
                            li {
                                TypeMenuItem {
                                    is_input,
                                    type_: type_.clone(),
                                    is_active: active_type_index() == Some(idx),
                                    on_hover: move |_| active_type_index.set(Some(idx)),
                                    top_level_menu,
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
pub fn TypeMenuItem(is_input: bool, type_: CWLType, is_active: bool, on_hover: EventHandler<()>, top_level_menu: Signal<bool>) -> Element {
    rsx! {
        div { class: "flex relative", onmouseenter: move |_| on_hover.call(()),

            div { class: "flex w-48 bg-fairagro-light-200/80 hover:bg-fairagro-light-400 px-2 py-1.25 items-center justify-end",
                "{type_}"
                div { class: "ml-auto",
                    Icon {
                        width: ICON_SIZE,
                        height: ICON_SIZE,
                        icon: GoChevronRight,
                    }
                }
            }

            if is_active {
                div { class: "absolute left-48 top-0",
                    NameInputForm {
                        is_input,
                        type_: type_.clone(),
                        top_level_menu,
                    }
                }
            }
        }
    }
}

#[component]
pub fn NameInputForm(is_input: bool, type_: CWLType, top_level_menu: Signal<bool>) -> Element {
    let mut app_state = use_app_state();
    let mut name_input = use_signal(String::new);

    rsx! {
        div { class: "min-w-48 bg-fairagro-light-200/80 px-2 py-0.25 hover:bg-fairagro-light-400 flex items-center gap-2",
            input {
                r#type: "text",
                class: "px-1 py-1 flex-1 focus:border-fairagro-dark-500 border-1",
                value: "{name_input}",
                oninput: move |e| name_input.set(e.value()),
                onclick: move |e| e.stop_propagation(),
                onkeydown: move |e| {
                    if e.key() == Key::Enter {
                        let id = name_input();
                        if !id.is_empty() {
                            if is_input {
                                app_state.write().workflow.add_input(&id, type_.clone())?;
                            } else {
                                app_state.write().workflow.add_output(&id, type_.clone())?;
                            }
                            name_input.set(String::new());
                        }
                        //we are sucessful and can close the whole menu
                        top_level_menu.set(false);
                    }
                    Ok(())
                },
                placeholder: "enter Input/Output id",
            }
        }
    }
}

fn type_iter() -> impl Iterator<Item = CWLType> {
    use CWLType::*;
    vec![Null, Boolean, Int, Long, Float, Double, String, File, Directory, Any, Stdout, Stderr].into_iter()
}
