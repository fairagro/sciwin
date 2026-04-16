use crate::layout::INPUT_TEXT_CLASSES;
use crate::layout::RELOAD_TRIGGER;
use crate::layout::Route;
use dioxus::prelude::*;
use dioxus_primitives::alert_dialog::*;
use s4n_core::{io::get_workflows_folder, workflow::create_workflow};
use std::path::{Path, PathBuf};
use rocrate::RocrateRunType;

#[component]
pub fn AbstractDialog(
    buttons: Element,
    title: String,
    children: Element,
    open: Signal<bool>,
    on_confirm: Option<EventHandler<MouseEvent>>,
    #[props(optional)] content_class: Option<String>,
) -> Element {
    let content_class = content_class.unwrap_or_else(|| {
        "select-none absolute justify-center bg-white top-1/2 left-1/2 transform -translate-x-1/2 -translate-y-1/2 rounded-sm min-w-64 shadow-xl border-1 border-fairagro-dark-500".to_string()
    });
    rsx! {
        AlertDialogRoot {
            class: "absolute h-screen w-screen left-0 top-0 overflow-hidden bg-zinc-500/60 z-900",
            open: open(),
            on_open_change: move |v| open.set(v),

            AlertDialogContent { class: "{content_class}",
                AlertDialogTitle {
                    class: "py-1 px-4 bg-fairagro-mid-500 rounded-t-sm font-bold center border-b-1 border-fairagro-dark-500",
                    "{title}"
                }
                AlertDialogDescription { class: "py-2 px-4", {children} }
                AlertDialogActions { class: "flex justify-center py-2 gap-2", {buttons} }
            }
        }
    }
}

#[component]
pub fn Dialog(
    title: String,
    children: Element,
    open: Signal<bool>,
    on_confirm: Option<EventHandler<MouseEvent>>,
    #[props(optional)] content_class: Option<String>,
) -> Element {
    rsx! {
        AbstractDialog {
            buttons: rsx! {
                AlertDialogAction {
                    class: "cursor-pointer border-1 border-fairagro-mid-500 rounded-sm px-4 py-1 hover:bg-fairagro-mid-500 hover:text-white",
                    on_click: on_confirm,
                    "Ok"
                }
                AlertDialogCancel {
                    class: "cursor-pointer border-1 border-fairagro-red-light rounded-sm px-4 py-1 hover:bg-fairagro-red-light hover:text-white",
                    "Cancel"
                }
            },
            title,
            open,
            on_confirm,
            content_class,
            children,
        }
    }
}

#[component]
pub fn OkDialog(title: String, children: Element, open: Signal<bool>, on_confirm: Option<EventHandler<MouseEvent>>) -> Element {
    rsx! {
        AbstractDialog {
            buttons: rsx! {
                AlertDialogAction {
                    class: "cursor-pointer border-1 border-fairagro-mid-500 rounded-sm px-4 py-1 hover:bg-fairagro-mid-500 hover:text-white",
                    on_click: on_confirm,
                    "Ok"
                }
            },
            title,
            open,
            on_confirm,
            children,
        }
    }
}

#[component]
pub fn WorkflowAddDialog(
    open: Signal<bool>,
    working_dir: ReadSignal<PathBuf>,
    show_add_actions: Signal<bool>,
) -> Element {
    let mut workflow_name = use_signal(|| "".to_string());

    let mut confirm = move || {
        create_workflow_impl(working_dir(), workflow_name())?;

        workflow_name.set("".to_string());
        show_add_actions.set(false);
        *RELOAD_TRIGGER.write() += 1;
        open.set(false);
        Ok::<_, anyhow::Error>(())
    };

    rsx! {
        Dialog {
            open,
            title: "Create new Workflow",
            on_confirm: move |_| {
                confirm()?;
                Ok(())
            },
            div { class: "flex flex-col",
                label { class: "text-fairagro-dark-500 font-bold", "Enter Workflow Name" }
                input {
                    class: "mt-2 w-full {INPUT_TEXT_CLASSES}",
                    value: "{workflow_name}",
                    r#type: "text",
                    placeholder: "workflow name ",
                    oninput: move |e| workflow_name.set(e.value()),
                    onkeydown: move |e| {
                        if e.key() == Key::Enter {
                            confirm()?;
                        }
                        Ok(())
                    },
                }
            }
        }
    }
}

fn create_workflow_impl(project_root: impl AsRef<Path>, name: String) -> anyhow::Result<()> {
    if name.is_empty() {
        anyhow::bail!("Workflow name was empty. Please enter a name!")
    }

    let path = project_root.as_ref().join(get_workflows_folder()).join(&name).join(format!("{name}.cwl"));
    create_workflow(&path, false)?;

    navigator().push(Route::WorkflowView {
        path: path.to_string_lossy().to_string(),
    });
    Ok(())
}

#[component]
pub fn NoProjectDialog(open: Signal<bool>, confirmed: Signal<bool>) -> Element {
    rsx! {
        Dialog {
            open,
            title: "No Project found!",
            on_confirm: move |_| {
                confirmed.set(true);
            },
            div {
                "There is no project that has been initialized in the folder you selected. Do you want to create a new project?"
            }
        }
    }
}

#[component]
pub fn ConfirmDialog(open: Signal<bool>, confirmed: Signal<bool>) -> Element {
    rsx! {
        Dialog {
            open,
            title: "Are you sure?",
            on_confirm: move |_| {
                confirmed.set(true);
            },
            div { "Are you sure you want to do that?" }
        }
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct MetadataDialogState {
    pub name: Option<String>,
    pub description: Option<String>,
    pub license: Option<String>,
    pub working_dir: Option<String>,
    pub output_dir: Option<String>,
    pub profile: RocrateRunType,
}

#[component]
pub fn MetadataDialog(
    state: MetadataDialogState,
    open: Signal<bool>,
    on_submit: EventHandler<MetadataDialogState>,
    #[props(optional)] on_close: Option<EventHandler<()>>,
) -> Element {
    const SPDX_URL: &str = "https://spdx.org/licenses/licenses.json";
    const INPUT: &str =
        "w-full p-2 mt-1 bg-zinc-900 border border-zinc-700 rounded text-white focus:outline-none focus:ring-1 focus:ring-green-500";
    const DROPDOWN: &str =
        "absolute mt-1 w-full bg-zinc-900 border border-zinc-700 rounded shadow-lg z-10";
    let mut name = use_signal(|| state.name.unwrap_or_default());
    let mut description = use_signal(|| state.description.unwrap_or_default());
    let mut license = use_signal(|| state.license.unwrap_or_default());
    let mut search = use_signal(|| license().clone());
    let mut profile = use_signal(|| state.profile);
    let mut licenses = use_signal(Vec::<(String, String)>::new);
    let mut show_profile_dropdown = use_signal(|| false);
    let mut show_license_dropdown = use_signal(|| false);
    // fetch SPDX licenses
    use_effect(move || {
        spawn(async move {
            if let Ok(resp) = reqwest::get(SPDX_URL).await
                && let Ok(json) = resp.json::<serde_json::Value>().await
                && let Some(arr) = json["licenses"].as_array()
            {
                let mut list: Vec<(String, String)> = arr
                    .iter()
                    .filter_map(|l| {
                        Some((
                            l.get("name")?.as_str()?.to_string(),
                            l.get("licenseId")?.as_str()?.to_string(),
                        ))
                    })
                    .collect();

                list.sort_by(|a, b| a.0.cmp(&b.0));
                licenses.set(list);
            }
        });
    });
    let filtered = {
        let q = search().to_lowercase();
        licenses()
            .iter()
            .filter(|(n, id)| {
                n.to_lowercase().contains(&q) || id.to_lowercase().contains(&q)
            })
            .take(20)
            .cloned()
            .collect::<Vec<_>>()
    };
    let profile_label = match profile() {
        RocrateRunType::WorkflowROCrate => "Workflow RO-Crate",
        RocrateRunType::ProvenanceRun => "Provenance Run Crate",
        RocrateRunType::WorkflowRun => "Workflow Run Crate",
        RocrateRunType::ProcessRun => "Process Run Crate",
        RocrateRunType::ArcROCrate => "ARC RO-Crate",
    };
    rsx! {
        Dialog {
            title: "Workflow Metadata".to_string(),
            open: open,
            content_class: Some(
                "select-none absolute justify-center bg-black text-white \
                top-1/2 left-1/2 transform -translate-x-1/2 -translate-y-1/2 \
                rounded-sm min-w-64 shadow-xl border border-zinc-700".to_string()
            ),
            on_confirm: Some(EventHandler::new(move |_| {
                on_submit.call(MetadataDialogState {
                    name: Some(name()),
                    description: Some(description()),
                    license: Some(license()),
                    working_dir: state.working_dir.clone(),
                    output_dir: state.output_dir.clone(),
                    profile: profile(),
                });

                if let Some(on_close) = &on_close {
                    on_close.call(());
                }
            })),
            div {
                class: "space-y-4 w-[420px] text-white bg-black -mx-4 -my-2 p-4",
                // profile
                div {
                    class: "relative",
                    label { class: "text-sm text-zinc-300", "RO-Crate Profile" }
                    input {
                        class: INPUT,
                        value: "{profile_label}",
                        readonly: true,
                        onclick: move |_| {
                            show_profile_dropdown.toggle();
                            show_license_dropdown.set(false);
                        }
                    }
                    if show_profile_dropdown() {
                        div { class: DROPDOWN,
                            for (label, value) in [
                                ("Workflow RO-Crate", RocrateRunType::WorkflowROCrate),
                                ("Provenance Run Crate", RocrateRunType::ProvenanceRun),
                                ("Workflow Run Crate", RocrateRunType::WorkflowRun),
                                ("Process Run Crate", RocrateRunType::ProcessRun),
                                ("ARC RO-Crate", RocrateRunType::ArcROCrate),
                            ] {
                                div {
                                    class: "px-3 py-2 cursor-pointer hover:bg-green-600",
                                    onclick: move |_| {
                                        profile.set(value);
                                        show_profile_dropdown.set(false);
                                    },
                                    "{label}"
                                }
                            }
                        }
                    }
                }
                // name
                div {
                    label { class: "text-sm text-zinc-300", "Name" }
                    textarea {
                        class: "{INPUT} h-20 resize-none",
                        value: "{name()}",
                        oninput: move |e| name.set(e.value())
                    }
                }
                // description
                div {
                    label { class: "text-sm text-zinc-300", "Description" }
                    textarea {
                        class: "{INPUT} h-20 resize-none",
                        value: "{description()}",
                        oninput: move |e| description.set(e.value())
                    }
                }
                // license
                div {
                    class: "relative",
                    label { class: "text-sm text-zinc-300", "License (SPDX)" }
                    input {
                        class: INPUT,
                        value: "{license()}",
                        placeholder: "MIT, Apache-2.0…",
                        oninput: move |e| {
                            let v = e.value();
                            license.set(v.clone());
                            search.set(v);
                            show_license_dropdown.set(true);
                        }
                    }
                    if show_license_dropdown() {
                        div { class: "{DROPDOWN} max-h-48 overflow-y-auto",
                            if filtered.is_empty() {
                                div { class: "px-3 py-2 text-zinc-500", "No licenses found" }
                            } else {
                                for (name_val, id_val) in filtered {
                                    div {
                                        key: "{id_val}",
                                        class: "px-3 py-2 cursor-pointer hover:bg-green-600 flex justify-between",
                                        onclick: move |_| {
                                            license.set(id_val.clone());
                                            search.set(id_val.clone());
                                            show_license_dropdown.set(false);
                                        },
                                        span { "{name_val}" }
                                        span { class: "text-zinc-400 text-sm", "{id_val}" }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}