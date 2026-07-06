use crate::components::ExecutionType;
use crate::components::files::{FileType, read_node_type};
use crate::components::files::{Node, get_route};
use crate::components::{ICON_SIZE, SmallRoundActionButton};
use crate::files::{get_cwl_files, get_submodules_cwl_files};
use crate::layout::{INPUT_TEXT_CLASSES, RELOAD_TRIGGER, Route};
use crate::reana_integration::{execute_reana_workflow, get_reana_credentials};
use crate::use_app_state;
use dioxus::core::spawn;
use dioxus_free_icons::Icon;
use dioxus_free_icons::icons::go_icons::{GoCloud, GoFileDirectory, GoPlay, GoPlusCircle, GoTrash};
use repository::Repository;
use repository::submodule::{add_submodule, remove_submodule};
use reqwest::Url;
use std::path::{Path, PathBuf};
use std::time::Duration;
use std::{env, fs};
use std::sync::Arc;
use commonwl::engine::{
    ContainerEngine, EngineStatus, InputObject, LocalBackend, create_execution_request,
    create_execution_request_with_inputs, evaluate_exitcodes,
};
use commonwl::storage::{StorageBackend, StoragePath};
use futures_util::StreamExt;
use tokio_util::sync::CancellationToken;
use dioxus::prelude::*;

#[component]
pub fn SolutionView(
    project_path: ReadSignal<PathBuf>,
    dialog_signals: (Signal<bool>, Signal<bool>),
) -> Element {
    let mut app_state = use_app_state();
    let files = use_memo(move || {
        RELOAD_TRIGGER();
        get_cwl_files(project_path().join("workflows"))
    });
    let submodule_files = use_memo(move || {
        RELOAD_TRIGGER();
        get_submodules_cwl_files(project_path())
    });
    let mut hover = use_signal(|| false);
    let mut adding = use_signal(|| false);
    let mut processing = use_signal(|| false);
    let mut new_package = use_signal(String::new);
    let show_settings = use_signal(|| false);
    let exec_coroutine = use_coroutine(move |mut rx: UnboundedReceiver<ExecutionRequest>| {
        let mut app_state = app_state;
        async move {
            while let Some(req) = rx.next().await {
                match req {
                    ExecutionRequest::Local { item, module } => {
                        let base_dir = match app_state().working_directory.clone() {
                            Some(dir) => dir,
                            None => {
                                eprintln!("No working directory");
                                continue;
                            }
                        };
                        let workflow_dir = if let Some(module_name) = module {
                            resolve_working_dir(base_dir.clone(), &Some(module_name))
                        } else {
                            base_dir.clone()
                        };
                        let cwl_path = match resolve_safe_cwl_path(&workflow_dir, Path::new(&item.path)) {
                            Ok(p) => p,
                            Err(e) => {
                                eprintln!("{e}");
                                continue;
                            }
                        };
                        let inputs_path = workflow_dir.join("inputs.yml");

                        app_state()
                            .terminal_log
                            .with_mut(|t| t.push_str("Starting local execution...\n"));

                        let outcome: anyhow::Result<()> = async {
                            let storage = Arc::new(StorageBackend::new());
                            let local_data_store = StoragePath::from_local(&env::temp_dir());
                            let backend = Arc::new(LocalBackend::new(
                                ContainerEngine::Docker,
                                storage,
                                local_data_store,
                            ));

                            let out_dir = Some(base_dir.as_path());
                            let request = if fs::exists(&inputs_path).unwrap_or(false) {
                                create_execution_request(
                                    cwl_path.clone(),
                                    inputs_path.to_string_lossy().into_owned(),
                                    out_dir,
                                )?
                            } else {
                                create_execution_request_with_inputs(
                                    cwl_path.clone(),
                                    InputObject::default(),
                                    out_dir,
                                    None,
                                )?
                            };

                            let cancellation_token = CancellationToken::new();
                            let result =
                                commonwl::engine::execute(backend, &request, cancellation_token).await?;
                            let evaluated_code =
                                evaluate_exitcodes(&result.exit_status, &request.specification);

                            match evaluated_code {
                                EngineStatus::Success(_) => Ok(()),
                                _ => anyhow::bail!("Execution failed"),
                            }
                        }
                        .await;

                        match outcome {
                            Ok(_) => {
                                app_state()
                                    .terminal_log
                                    .with_mut(|t| t.push_str("✅ Execution completed\n"));
                            }
                            Err(e) => {
                                app_state()
                                    .terminal_log
                                    .with_mut(|t| t.push_str(&format!("❌ Execution failed: {e:#}\n")));
                            }
                        }
                    }
                    ExecutionRequest::Remote { item, module, show_settings } => {
                        if get_reana_credentials().ok().flatten().is_none() {
                            app_state.write().show_manage_reana_modal.set(true);
                            app_state()
                                .terminal_log
                                .with_mut(|t| t.push_str("⚠ REANA credentials not configured.\n"));
                            continue;
                        }
                        let base_dir = match app_state().working_directory.clone() {
                            Some(dir) => dir,
                            None => {
                                app_state()
                                    .terminal_log
                                    .with_mut(|t| t.push_str("❌ No working directory configured.\n"));
                                continue;
                            }
                        };
                        let workflow_dir = if let Some(module_name) = &module {
                            resolve_working_dir(base_dir.clone(), &Some(module_name.clone()))
                        } else {
                            base_dir.clone()
                        };
                        let mut terminal_signal = app_state().terminal_log;
                        let (tx, mut rx2) = tokio::sync::mpsc::channel::<String>(100);

                        spawn(async move {
                            while let Some(msg) = rx2.recv().await {
                                terminal_signal.with_mut(|t| t.push_str(&msg));
                            }
                        });

                        if let Err(e) = execute_reana_workflow(item, workflow_dir, show_settings, Some(tx)).await {
                            app_state()
                                .terminal_log
                                .with_mut(|t| t.push_str(&format!("\n❌ Execution failed: {e}\n")));
                        }
                    }
                }
            }
        }
    });

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
                            div { class: if hover() { "flex items-center gap-1" } else { "flex items-center gap-1 invisible" },
                                SmallRoundActionButton {
                                    class: "hover:bg-fairagro-red-light",
                                    title: "Delete {item.name}",
                                    onclick: {
                                        let item = item.clone();
                                        move |_| {
                                            let item = item.clone();
                                            async move {
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
                                LocalRunButton {
                                    item: item.clone(),
                                    module: Some(
                                        item
                                            .path
                                            .parent()
                                            .and_then(|p| p.strip_prefix(project_path()).ok())
                                            .map(|p| p.to_string_lossy().to_string())
                                            .unwrap_or_default(),
                                    ),
                                    exec_coroutine,
                                }
                                if read_node_type(&item.path) == FileType::Workflow {
                                    RemoteRunButton {
                                        item: item.clone(),
                                        show_settings,
                                        module: None,
                                        exec_coroutine,
                                    }
                                }
                            }
                        }
                    }
                }
                for (module , files) in submodule_files() {
                    Submodule_View {
                        module,
                        files,
                        dialog_signals,
                        exec_coroutine,
                    }
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

                                let working_dir = project_path();
                                let mut repo = Repository::open(&working_dir)?;
                                let package = new_package();
                                let url = package.strip_suffix(".git").unwrap_or(&package);
                                let url_obj = Url::parse(url)?;
                                let package_dir = Path::new("packages");
                                let repo_name = url_obj.path().strip_prefix("/").unwrap();
                                add_submodule(
                                    &mut repo,
                                    url,
                                    &None,
                                    &working_dir.join(package_dir.join(repo_name)),
                                )?;
                                *RELOAD_TRIGGER.write() += 1;
                                processing.set(false);
                            }
                            Ok(())
                        },
                    }
                } else {
                    "..."
                }
            }
        }
    }
}

#[component]
pub fn Submodule_View(
    module: String,
    files: Vec<Node>,
    dialog_signals: (Signal<bool>, Signal<bool>),
    exec_coroutine: Coroutine<ExecutionRequest>,
) -> Element {
    let app_state = use_app_state();
    let mut hover = use_signal(|| false);
    let show_settings = use_signal(|| false);
    let module_for_buttons = module.clone();
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
                        let app_state = app_state;
                        async move {
                            dialog_signals.0.set(true);
                            loop {
                                if !dialog_signals.0() {
                                    if dialog_signals.1() {
                                        let working_dir = {
                                            let state = app_state();
                                            state.working_directory.clone()
                                        };
                                        if let Some(dir) = working_dir {
                                            let repo = Repository::open(dir)?;
                                            remove_submodule(&repo, &module)?;
                                            *RELOAD_TRIGGER.write() += 1;
                                        }
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
                        ondragstart: {
                            let mut app_state = app_state;
                            let item = item.clone();
                            move |e| {
                                e.data_transfer().set_effect_allowed("all");
                                e.data_transfer().set_drop_effect("move");
                                {
                                    let mut state = app_state.write();
                                    state.set_data_transfer(&item)?;
                                }
                                e.data_transfer()
                                    .set_data("application/x-allow-dnd", "1")
                                    .map_err(|e| anyhow::anyhow!("{e}"))?;
                                Ok(())
                            }
                        },
                        div { class: "flex gap-1 items-center",
                            Link {
                                draggable: "false",
                                to: get_route(&item),
                                active_class: "font-bold",
                                div {
                                    class: "flex",
                                    style: "width: {ICON_SIZE.unwrap()}px; height: {ICON_SIZE.unwrap()}px;",
                                    img { src: asset!("/assets/CWL.svg") }
                                }
                                "{item.name}"
                            }
                            LocalRunButton {
                                item: item.clone(),
                                module: module_for_buttons.clone(),
                                exec_coroutine,
                            }
                            if read_node_type(&item.path) == FileType::Workflow {
                                RemoteRunButton {
                                    item: item.clone(),
                                    show_settings,
                                    module: Some(module_for_buttons.clone()),
                                    exec_coroutine,
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
fn RemoteRunButton(
    item: Node,
    show_settings: Signal<bool>,
    module: Option<String>,
    exec_coroutine: Coroutine<ExecutionRequest>,
) -> Element {
    let mut app_state = use_app_state();
    rsx! {
        SmallRoundActionButton {
            class: "hover:bg-fairagro-mid-500",
            title: "Execute with REANA",
            onclick: move |_| {
                let mut state = app_state.write();
                navigator().push(Route::GlobalTerminal);
                state.active_tab.set("terminal".to_string());
                state.show_terminal_log.set(true);
                state.terminal_log.set("Starting remote execution...\n".to_string());
                state.terminal_exec_type.set(ExecutionType::Remote);
                drop(state);

                exec_coroutine
                    .send(ExecutionRequest::Remote {
                        item: item.clone(),
                        module: module.clone(),
                        show_settings,
                    });
            },
            Icon { width: 10, height: 10, icon: GoCloud }
        }
    }
}

#[component]
fn LocalRunButton(
    item: Node,
    module: Option<String>,
    exec_coroutine: Coroutine<ExecutionRequest>,
) -> Element {
    let mut app_state = use_app_state();
    rsx! {
        SmallRoundActionButton {
            class: "hover:bg-fairagro-mid-500",
            title: "Run locally",
            onclick: move |_| {
                let mut state = app_state.write();
                navigator().push(Route::GlobalTerminal);
                state.active_tab.set("terminal".to_string());
                state.show_terminal_log.set(true);
                state.terminal_log.set(String::new());
                state.terminal_exec_type.set(ExecutionType::Local);
                drop(state);

                exec_coroutine
                    .send(ExecutionRequest::Local {
                        item: item.clone(),
                        module: module.clone(),
                    });
            },
            Icon { width: 10, height: 10, icon: GoPlay }
        }
    }
}

fn resolve_working_dir(base: PathBuf, module: &Option<String>) -> PathBuf {
    if let Some(module) = module {
        base.join(module)
    } else {
        base
    }
}

pub fn resolve_safe_cwl_path(base_dir: &Path, candidate: &Path) -> Result<PathBuf, String> {
    let base = base_dir
        .canonicalize()
        .map_err(|e| format!("Failed to canonicalize base directory {:?}: {e}", base_dir))?;
    let joined = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        base.join(candidate)
    };
    let resolved = joined
        .canonicalize()
        .map_err(|e| format!("Failed to canonicalize CWL path {:?}: {e}", candidate))?;
    if !resolved.starts_with(&base) {
        return Err(format!(
            "❌ Unsafe CWL path: {:?} is outside the working directory {:?}",
            resolved, base
        ));
    }
    if !resolved.exists() {
        return Err(format!("❌ CWL file not found: {:?}", resolved));
    }
    Ok(resolved)
}

#[derive(Clone)]
pub enum ExecutionRequest {
    Local {
        item: Node,
        module: Option<String>,
    },
    Remote {
        item: Node,
        module: Option<String>,
        show_settings: Signal<bool>,
    },
}