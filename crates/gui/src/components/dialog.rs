use crate::layout::INPUT_TEXT_CLASSES;
use crate::layout::RELOAD_TRIGGER;
use crate::layout::Route;
use dioxus::prelude::*;
use dioxus_primitives::alert_dialog::*;
use s4n_core::{io::get_workflows_folder, workflow::create_workflow};
use std::path::{Path, PathBuf};

#[component]
pub fn AbstractDialog(
    buttons: Element,
    title: String,
    children: Element,
    open: Signal<bool>,
    on_confirm: Option<EventHandler<MouseEvent>>,
) -> Element {
    rsx! {
        AlertDialogRoot {
            class: "absolute h-screen w-screen left-0 top-0 overflow-hidden bg-zinc-500/60 z-900",
            open: open(),
            on_open_change: move |v| open.set(v),
            AlertDialogContent { class: "select-none absolute justify-center bg-white top-1/2 left-1/2 transform -translate-x-1/2 -translate-y-1/2 rounded-sm min-w-64 shadow-xl border-1 border-fairagro-dark-500",
                AlertDialogTitle { class: "py-1 px-4 bg-fairagro-mid-500 rounded-t-sm font-bold center border-b-1 border-fairagro-dark-500",
                    "{title}"
                }
                AlertDialogDescription { class: "py-2 px-4", {children} }
                AlertDialogActions { class: "flex justify-center py-2 gap-2", {buttons} }
            }
        }
    }
}

#[component]
pub fn Dialog(title: String, children: Element, open: Signal<bool>, on_confirm: Option<EventHandler<MouseEvent>>) -> Element {
    rsx! {
        AbstractDialog {
            buttons: rsx! {
                AlertDialogAction {
                    class: "cursor-pointer border-1 border-fairagro-mid-500 rounded-sm px-4 py-1 hover:bg-fairagro-mid-500 hover:text-white",
                    on_click: on_confirm,
                    "Ok"
                }
                AlertDialogCancel { class: "cursor-pointer border-1 border-fairagro-red-light rounded-sm px-4 py-1 hover:bg-fairagro-red-light hover:text-white",
                    "Cancel"
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
