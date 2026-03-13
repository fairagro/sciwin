use crate::{
    ApplicationState,
    components::{
        CodeViewer, ConfirmDialog, ICON_SIZE, NoProject, NoProjectDialog, OkDialog, RoundActionButton, SmallRoundActionButton, ToolAddForm,
        WorkflowAddDialog,
        files::{FilesView, View},
        graph::GraphEditor,
        layout::{Footer, Main, Sidebar, TabContent, TabList, TabTrigger, Tabs},
        GlobalTerminal,
    },
    last_session_data, open_project, restore_last_session, save_file, use_app_state,
};
use dioxus::prelude::*;
use dioxus_free_icons::{
    Icon,
    icons::go_icons::{GoAlert, GoGitCommit, GoPlus, GoRepo, GoSync, GoWorkflow, GoX, GoTerminal},
};
use rfd::AsyncFileDialog;
use std::{fs, path::PathBuf};

pub const INPUT_TEXT_CLASSES: &str = "shadow appearance-none border rounded py-2 px-3 text-zinc-700 leading-tight focus:border-fairagro-dark-500 focus:outline-none focus:shadow-outline";
pub static RELOAD_TRIGGER: GlobalSignal<i32> = Signal::global(|| 0);

#[component]
pub fn Layout() -> Element {
    let mut app_state = use_app_state();
    let working_dir = use_memo(move || app_state.read().working_directory.clone());

    let mut view = use_signal(|| View::Solution);

    let route: Route = use_route();
    let mut route_rx = use_reactive(&route, |route| route);

    let mut show_add_actions = use_signal(|| false);
    let mut show_create_dialog = use_signal(|| false);

    let show_project_dialog = use_signal(|| false);
    let confirm_project_dialog = use_signal(|| false);

    let show_confirm_dialog = use_signal(|| false);
    let confirmed_dialog = use_signal(|| false);

    let mut is_editing_name = use_signal(|| false);
    let mut new_project_name = use_signal(String::new);

    {
        use_effect(move || {
            if *is_editing_name.read() {
                //we should be save to unwrap in this effect, hopefully
                new_project_name.set(app_state().config.unwrap().workflow.name);
            }
        });
    }

    {
        use_effect(move || {
            app_state.write().current_file = match route_rx() {
                Route::WorkflowView { path } => Some(PathBuf::from(path)),
                Route::ToolView { path } => Some(PathBuf::from(path)),
                _ => None,
            };

            let serialized = serde_json::to_string(&app_state()).expect("Could not serialize app state");
            fs::write(last_session_data(), serialized).expect("Could not save app state");
        });
    }

    rsx! {
        div {
            class: "h-screen w-screen grid grid-rows-[1fr_1.5rem]",
            onmounted: move |_| async move {
                spawn(async move {
                    if let Some(last_session) = restore_last_session(
                            show_project_dialog,
                            confirm_project_dialog,
                        )
                        .await
                        .unwrap()
                    {
                        app_state.set(last_session)
                    }
                });
                Ok(())
            },
            div { class: "flex min-h-0 h-full w-full overflow-x-clip relative",
                Sidebar {
                    if let Some(config) = &app_state.read().config {
                        h2 { class: "text-fairagro-dark-500 mb-2 text-sm flex items-center gap-1.5",
                            Icon { icon: GoRepo, width: 16, height: 16 }
                            if is_editing_name() {
                                input {
                                    class: "shadow appearance-none border rounded w-full py-1 px-3 leading-tight focus:border-fairagro-dark-500 focus:outline-none focus:shadow-outline",
                                    r#type: "text",
                                    value: "{new_project_name}",
                                    oninput: move |e| new_project_name.set(e.value()),
                                    onkeypress: move |e| {
                                        if e.key() == Key::Enter {
                                            let working_dir = app_state().working_directory.unwrap();
                                            if let Some(write_config) = &mut app_state.write().config {
                                                e.stop_propagation();
                                                write_config.workflow.name = new_project_name();
                                                let toml = write_config.to_toml()?;
                                                fs::write(working_dir.join("workflow.toml"), toml)?;
                                                save_file(&working_dir, "workflow.toml", "🧾 Changed project name")?;

                                                is_editing_name.set(false);
                                            }
                                        }
                                        if e.key() == Key::Escape {
                                            is_editing_name.set(false);
                                        }
                                        Ok(())
                                    },
                                    placeholder: "Project name",
                                }
                            } else {
                                div {
                                    ondoubleclick: move |e| {
                                        e.prevent_default();
                                        is_editing_name.set(true);
                                    },
                                    title: "Double-click to edit",
                                    "{config.workflow.name}"
                                }
                            }
                            SmallRoundActionButton {
                                class: "hover:bg-fairagro-red-light/20 text-fairagro-red",
                                title: "Close Project",
                                onclick: move |_| {
                                    fs::remove_file(last_session_data())?;
                                    app_state.set(ApplicationState::default());
                                    router().push("/");
                                    Ok(())
                                },
                                Icon {
                                    icon: GoX,
                                    width: ICON_SIZE,
                                    height: ICON_SIZE,
                                }
                            }
                            SmallRoundActionButton {
                                class: "hover:bg-fairagro-dark-500/20 text-fairagro-dark-500",
                                title: "Reload Files",
                                onclick: move |_| {
                                    *RELOAD_TRIGGER.write() += 1;
                                },
                                Icon {
                                    icon: GoSync,
                                    width: ICON_SIZE,
                                    height: ICON_SIZE,
                                }
                            }
                        }
                    } else {
                        form {
                            onsubmit: move |e| {
                                e.prevent_default();
                                spawn(async move {
                                    let path = AsyncFileDialog::new().pick_folder().await.unwrap();
                                    if let Some(info) = open_project(
                                            path.path(),
                                            show_project_dialog,
                                            confirm_project_dialog,
                                        )
                                        .await
                                        .unwrap()
                                    {
                                        app_state.write().working_directory = Some(info.working_directory);
                                        app_state.write().config = Some(info.config);
                                    }
                                    //move to home if new project opens
                                    view.set(View::Solution);
                                    router().push("/");
                                });
                                Ok(())
                            },
                            input {
                                r#type: "submit",
                                value: "Load Project",
                                class: "rounded-lg bg-fairagro-light-500 px-3 py-1 cursor-pointer",
                            }
                        }
                    }
                    if let Some(working_dir) = working_dir() {
                        select {
                            onchange: move |e| view.set(e.value().parse().unwrap()),
                            class: "form-select appearance-none rounded-base bg-zinc-300 w-full px-2 py-1.5 font-bold bg-no-repeat",
                            option { value: "Solution", "Solution" }
                            option { value: "FileSystem", "Filesystem" }
                        }
                        FilesView {
                            working_dir,
                            view,
                            dialog_signals: (show_confirm_dialog, confirmed_dialog),
                        }
                    } else {
                        NoProject {}
                    }
                }
                ErrorBoundary {
                    handle_error: |errors: ErrorContext| {
                        let errors_clone = errors.clone();
                        let mut open = use_signal(|| true);
                        let error = errors.error();

                        rsx! {
                            OkDialog {
                                title: "An Error occured",
                                open,
                                on_confirm: move |_| {
                                    errors_clone.clear_errors();
                                    // we set back to true as it would be invisible on next error
                                    open.set(true);
                                },
                                div { class: "flex gap-4",
                                    Icon { width: 32, height: 32, icon: GoAlert }
                                    if let Some(error) = error {
                                        div {
                                            p { "Oops, we encountered an error." }
                                            p { class: "font-bold text-fairagro-red", "{error}" }
                                        }
                                    }
                                }
                            }
                        }
                    },
                    Main { Outlet::<Route> {} }
                    //floating action button
                    if let Some(working_dir) = working_dir() {
                        WorkflowAddDialog {
                            open: show_create_dialog,
                            working_dir,
                            show_add_actions,
                        }
                    }
                    NoProjectDialog {
                        open: show_project_dialog,
                        confirmed: confirm_project_dialog,
                    }
                    ConfirmDialog {
                        open: show_confirm_dialog,
                        confirmed: confirmed_dialog,
                    }
                    div { class: "z-100 bg-fairagro-mid-200 absolute right-10 bottom-10 rounded-full flex flex-col",
                        if *show_add_actions.read() {
                            div { class: "flex relative mb-3",
                                div { class: "absolute text-center right-11 top-3 py-0.5 px-1 bg-fairagro-dark-500 rounded-md text-[.6rem] text-white ring-1 ring-fairagro-dark-300/40",
                                    "Workflow"
                                }
                                RoundActionButton {
                                    title: "Add new Workflow",
                                    onclick: move |_| {
                                        show_add_actions.set(false);
                                        show_create_dialog.set(true)
                                    },
                                    Icon {
                                        width: 16,
                                        height: 16,
                                        icon: GoWorkflow,
                                    }
                                }
                            }
                            div { class: "flex relative mb-3",
                                div { class: "absolute text-center right-11 top-3 py-0.5 px-1 bg-fairagro-dark-500 rounded-md text-[.6rem] text-white ring-1 ring-fairagro-dark-300/40",
                                    "Tool"
                                }
                                RoundActionButton {
                                    title: "Add new CommandLineTool",
                                    onclick: move |_| {
                                        show_add_actions.set(false);
                                        navigator().push(Route::ToolAdd);
                                    },
                                    Icon {
                                        width: 16,
                                        height: 16,
                                        icon: GoGitCommit,
                                    }
                                }
                            }
                        }
                        div { class: "flex relative mb-3",
                            RoundActionButton {
                                title: "Add new CWL File",
                                onclick: move |_| { show_add_actions.set(!show_add_actions()) },
                                Icon { width: 16, height: 16, icon: GoPlus }
                            }
                        }
                        div { class: "flex relative mb-3",
                            RoundActionButton {
                                title: "Open Terminal",
                                onclick: move |_| { navigator().push(Route::GlobalTerminal); },
                                Icon { width: 16, height: 16, icon: GoTerminal }
                            }
                        }
                    }
                }
            }
            Footer {
                match &route {
                    Route::WorkflowView { path } => path.to_string(),
                    Route::ToolView { path } => path.to_string(),
                    _ => "".to_string(),
                }
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Routable)]
pub enum Route {
    #[layout(Layout)]
    #[route("/")]
    Empty,

    #[route("/workflow?:path")]
    WorkflowView { path: String },

    #[route("/tool?:path")]
    ToolView { path: String },

    #[route("/tool_add")]
    ToolAdd,

    #[route("/global_terminal")]
    GlobalTerminal,
}

#[component]
pub fn Empty() -> Element {
    rsx!(
        div {}
    )
}

#[component]
pub fn WorkflowView(path: String) -> Element {
    rsx!(
        Tabs { class: "h-full min-h-0", default_value: "editor".to_string(),
            TabList {
                TabTrigger { index: 0usize, value: "editor".to_string(), "Nodes" }
                TabTrigger { index: 1usize, value: "code".to_string(), "Code" }
            }
            TabContent {
                index: 0usize,
                class: "h-full min-h-0",
                value: "editor".to_string(),
                GraphEditor { path: path.clone() }
            }
            TabContent {
                index: 1usize,
                class: "h-full min-h-0",
                value: "code".to_string(),
                CodeViewer { path: path.clone() }
            }
        }
    )
}

#[component]
pub fn ToolView(path: String) -> Element {
    rsx! {
        Tabs { class: "h-full min-h-0", default_value: "code".to_string(),
            TabList {
                TabTrigger { index: 0usize, value: "code".to_string(), "Code" }
            }
            TabContent {
                index: 0usize,
                class: "h-full min-h-0",
                value: "code".to_string(),
                CodeViewer { path }
            }
        }
    }
}


#[component]
pub fn ToolAdd() -> Element {
    rsx! {
        ToolAddForm {}
    }
}
