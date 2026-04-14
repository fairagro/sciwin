use crate::{
    components::{ICON_SIZE, NonRotatingActionButton, ToastItem, MetadataDialog, MetadataDialogState, files::{FileType, read_node_type}},
    use_app_state,
};
use dioxus::prelude::*;
use dioxus_free_icons::{
    Icon,
    icons::go_icons::{GoDownload, GoCloud, GoPackage},
};
use serde::{Deserialize, Serialize};
use crate::reana_integration::{get_reana_credentials, store_reana_credentials, delete_reana_credentials, get_last_workflow_name};
use crate::layout::Route;
use anyhow::{anyhow, Result};
use commonwl::annotation::common::{annotate_license, annotate_field};
use std::{time::Duration, sync::Arc};
use rocrate::{export_rocrate, RocrateRunType, extract_metadata_from_cwl_file};
use commonwl::{load_doc, CWLDocument};
use reana::{api::{get_workflow_specification, get_workflow_logs}, reana::Reana, utils::get_cwl_name};
use cwl_core::{packed::pack_workflow};

#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum ExecutionType {
    #[default]
    Remote,
    Local,
}

#[component]
fn TerminalHeader(exec_type: ExecutionType) -> Element {
    let navigator = use_navigator();
    let title = match exec_type {
        ExecutionType::Local => "Local Execution",
        ExecutionType::Remote => "Remote Execution",
    };
    rsx! {
        div {
            class: "flex justify-between items-center bg-zinc-900 text-white px-3 py-2",
            h2 { class: "text-sm font-semibold", "{title}" }
            button {
                class: "text-red-400 hover:text-red-600",
                onclick: move |_| {
                    navigator.push(Route::Empty);
                },
                "✕"
            }
        }
    }
}

#[derive(Clone, PartialEq)]
pub enum ExportState {
    Idle,
    WaitingForMetadata(MetadataDialogState),
    Running,
    Success,
    Error(String),
}

#[component]
fn RemoteActions(export_state: Signal<ExportState>, toast_items: Signal<Vec<ToastItem>>, instance: String, token: String) -> Element {
    let show_modal = use_app_state()().show_manage_reana_modal;
    let reana_instance = Arc::new(reana::reana::Reana::new(instance.clone(), token.clone()));
    rsx! {
        div {
            class: "flex items-center space-x-2 px-3 py-2 bg-zinc-800 text-white",
            DownloadResultsButton { toast_items }
            ExportRoCrateButton {
                export_state,
                toast_items,
                exec_type: ExecutionType::Remote,
                reana: Some(reana_instance),
            }
            ManageReanaButton { show_modal: Some(show_modal) }
        }
    }
}

#[component]
pub fn LocalActions(export_state: Signal<ExportState>, toast_items: Signal<Vec<ToastItem>>, is_workflow: bool) -> Element {
    if !is_workflow {
        return rsx! {};
    }
    rsx! {
        div {
            class: "flex items-center space-x-2 px-3 py-2 bg-zinc-800 text-white",
            ExportRoCrateButton {
                export_state,
                toast_items,
                exec_type: ExecutionType::Local,
                reana: None,
            }
        }
    }
}

#[component]
fn DownloadResultsButton(toast_items: Signal<Vec<ToastItem>>) -> Element {
    let app_state = use_app_state();
    rsx! {
        NonRotatingActionButton {
            onclick: move |_| {
                toast_items.write().push(
                    ToastItem::new(
                        "Download Results".into(),
                        "Downloading files...".into(),
                        3
                    )
                );
                let output_dir = app_state().working_directory.as_ref().map(|p| p.to_string_lossy().to_string());
                spawn(async move {
                    let workflow_name = match get_last_workflow_name().await {
                        Ok(n) => n,
                        Err(e) => {
                            eprintln!("workflow error {e}");
                            return;
                        }
                    };
                    let _ = remote_execution::download_results(&workflow_name, false, output_dir.as_ref()).await.map_err(|e| e.to_string());
                });
            },
            div {
                class: "flex items-center space-x-2",
                Icon { icon: GoDownload, width: ICON_SIZE, height: ICON_SIZE }
                span { "Download Results" }
            }
        }
    }
}

pub async fn extract_metadata(workflow_name: Option<&str>, reana: Option<Arc<reana::reana::Reana>>, cwl_file: Option<&str>) -> Result<(Option<String>, Option<String>, Option<String>)> {
    let cwl_file = if let Some(file) = cwl_file {
        file.to_string()
    } else if let Some(reana) = reana.clone() {
        let workflow_name = workflow_name.ok_or_else(|| anyhow!("workflow_name required"))?;
        get_cwl_name(Some(reana), workflow_name).await?
    } else {
        return Err(anyhow!("CWL file or REANA instance is required"));
    };
    extract_metadata_from_cwl_file(&cwl_file)
}

#[component]
fn ExportRoCrateButton(export_state: Signal<ExportState>, toast_items: Signal<Vec<ToastItem>>, exec_type: ExecutionType, reana: Option<Arc<reana::reana::Reana>>) -> Element {
    let app_state = use_app_state();
    rsx! {
        NonRotatingActionButton {
            disabled: matches!(export_state(), ExportState::Running),
            onclick: {
                let exec_type = exec_type.clone();
              //  let app_state = app_state;
                let export_state = export_state;
                let reana = reana.clone();
                EventHandler::new(move |_| {
                    if matches!(export_state(), ExportState::Running) {
                        return;
                    }
                    let exec_type = exec_type.clone();
                    let app_state = app_state;
                    let mut export_state = export_state;
                    let reana_clone = reana.clone();
                    spawn(async move {
                        let working_dir = app_state().working_directory.as_ref().map(|p| p.to_string_lossy().to_string());
                        let output_dir = working_dir.clone().map(|d| format!("{d}/rocrate"));
                        let (name, description, license, _cwl_file): (Option<String>, Option<String>, Option<String>, Option<String>) =
                            match exec_type {
                                ExecutionType::Local => {
                                    let cwl_file = (app_state().last_local_execution_file)().map(|p| p.to_string_lossy().to_string());
                                    if let Some(ref cwl_path) = cwl_file {
                                        match extract_metadata(None, None, Some(cwl_path)).await {
                                            Ok((n, d, l)) => (n, d, l, cwl_file),
                                            Err(e) => {
                                                eprintln!("Failed to extract local CWL metadata: {e}");
                                                (Some("".to_string()), Some("".to_string()), Some("not specified".to_string()), cwl_file)
                                            }
                                        }
                                    } else {
                                        (Some("".to_string()), Some("".to_string()), Some("not specified".to_string()), None)
                                    }
                                }
                                ExecutionType::Remote => {
                                    let reana = match reana_clone.clone() {
                                        Some(r) => r,
                                        None => {
                                            export_state.set(ExportState::Error("Missing REANA instance".into()));
                                            return;
                                        }
                                    };
                                    let workflow_name = match get_last_workflow_name().await {
                                        Ok(n) => n,
                                        Err(e) => {
                                            export_state.set(ExportState::Error(e.to_string()));
                                            return;
                                        }
                                    };
                                    let cwl_file = get_cwl_name(Some(reana.clone()), &workflow_name).await.ok();
                                    let cwl_file_clone = cwl_file.clone();
                                    let w_dir = app_state().working_directory.as_ref().map(|p| p.to_path_buf());
                                    let cwl = w_dir
                                        .and_then(|dir| cwl_file_clone.map(|file| dir.join(file)))
                                        .unwrap_or_else(|| std::path::PathBuf::from("unknown_cwl.cwl"));
                                    let (n, d, l) = extract_metadata(
                                        Some(&workflow_name),
                                        Some(reana.clone()),
                                        Some(cwl.to_string_lossy().as_ref()),
                                    )
                                    .await
                                    .unwrap_or((None, None, None));
                                    (n, d, l, Some(cwl.to_string_lossy().as_ref().to_string()))
                                }
                            };
                        let metadata = MetadataDialogState {
                            name,
                            description,
                            license,
                            working_dir,
                            output_dir,
                            profile: RocrateRunType::ProvenanceRun,
                        };
                        export_state.set(ExportState::WaitingForMetadata(metadata.clone()));
                    });
                })
            },
            div {
                class: "flex items-center space-x-2",
                Icon { icon: GoPackage, width: ICON_SIZE, height: ICON_SIZE }
                span { "Export RO-Crate" }
            }
        }
    }
}

#[component]
fn TerminalOutput(log: String) -> Element {
    rsx! {
        div {
            class: "flex-1 overflow-y-auto bg-black text-green-400 text-xs font-mono p-3 whitespace-pre-wrap",
            pre { "{log}" }
        }
    }
}

#[component] 
pub fn ManageReanaButton(#[props(optional)] show_modal: Option<Signal<bool>>) -> Element {
    let mut show_modal = show_modal.unwrap_or(use_signal(|| false));
    let mut instances = use_signal(Vec::<Reana>::new);
    let mut new_url = use_signal(String::new);
    let mut new_token = use_signal(String::new);
    let mut toast_items = use_context::<Signal<Vec<ToastItem>>>();
    {
        use_effect(move || {
            match get_reana_credentials() {
                Ok(Some((url, token))) => {
                    instances.write().push(Reana::new(url, token));
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
            rsx! {
                p { class: "text-gray-400", "No instances configured." }
            }
        } else {
            rsx! {
                ul {
                    class: "space-y-2",
                    {list.iter().enumerate().map(|(i, instance)| {
                        rsx! {
                            li {
                                key: "{i}",
                                class: "flex justify-between items-center border border-gray-700 p-2 rounded bg-zinc-800",
                                div { span { class: "font-medium text-white", "{instance.server()}" } }
                                button {
                                    class: "text-red-600 hover:text-red-800 px-2 py-1 rounded",
                                    onclick: move |_| {
                                        if let Err(e) = delete_reana_credentials() {
                                            push_toast("Error", format!("Failed to delete credentials: {e}"), 3);
                                        } else {
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
        NonRotatingActionButton {
            onclick: move |_| {
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
                class: "fixed top-20 right-20 bg-zinc-900 text-white p-6 rounded-lg shadow-lg w-96 space-y-4 z-50",
                h2 { class: "text-lg font-bold mb-2", "Configured REANA Instances" }
                {instance_list}
                div {
                    class: "space-y-2 border-t border-gray-700 pt-4",
                    h3 { class: "font-semibold text-white", "Add New Instance" }
                    input {
                        class: "w-full border border-gray-700 rounded px-2 py-1 bg-zinc-800 text-white placeholder-gray-400",
                        placeholder: "Instance URL",
                        value: "{new_url()}",
                        oninput: move |e| new_url.set(e.value()),
                    }
                    input {
                        class: "w-full border border-gray-700 rounded px-2 py-1 bg-zinc-800 text-white placeholder-gray-400",
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
                                        instances.write().push(Reana::new(new_url(), new_token()));
                                        new_url.set(String::new());
                                        new_token.set(String::new());
                                    }
                                }
                            },
                            "Add Instance"
                        }
                        button {
                            class: "bg-gray-700 text-white px-3 py-1 rounded hover:bg-gray-600",
                            onclick: move |_| show_modal.set(false),
                            "Close"
                        }
                    }
                }
            }
        }
    }
}

async fn run_export(metadata: MetadataDialogState, mut export_state: Signal<ExportState>, cwl_path: Option<String>,
    instance: Option<String>, token: Option<String>, exec_type: ExecutionType) -> Result<(), String> {
    match exec_type {
        ExecutionType::Local => {
            let cwlfile = cwl_path
                .clone()
                .ok_or_else(|| "Missing local CWL file".to_string())?;
            let doc = load_doc(&cwlfile).map_err(|e| e.to_string())?;
            let CWLDocument::Workflow(workflow) = doc else {
                return Err("CWL not workflow".into());
            };
            let packed = pack_workflow(&workflow, &cwlfile, None)
                .map_err(|e| e.to_string())?;
            let packed_json = serde_json::to_value(&packed)
                .map_err(|e| e.to_string())?;
            let graph_json = packed_json
                .get("$graph")
                .and_then(|v| v.as_array())
                .ok_or_else(|| "Missing graph".to_string())?;
            let working_dir = metadata.working_dir.clone().unwrap_or_default();
            export_rocrate(
                Some(&"rocrate".to_string()),
                Some(&working_dir),
                &cwlfile,
                metadata.profile,
                Some("local"),
                graph_json,
                None,
            )
            .await
            .map_err(|e| e.to_string())?
        }
        ExecutionType::Remote => {
            let instance = instance.ok_or_else(|| "Missing instance".to_string())?;
            let token = token.ok_or_else(|| "Missing token".to_string())?;
            let reana = Reana::new(instance, token);
            let workflow_name = match get_last_workflow_name().await {
                Ok(n) => n,
                Err(e) => {
                    export_state.set(ExportState::Error(e.to_string()));
                    return Err(e.to_string());
                }
            };
            let graph = get_workflow_specification(&reana, &workflow_name)
                .await
                .map_err(|e| e.to_string())?;
            let logs = get_workflow_logs(&reana, &workflow_name)
                .await
                .map_err(|e| e.to_string())?;
            let graph_array = graph
                .get("specification")
                .and_then(|spec| spec.get("workflow"))
                .and_then(|s| s.get("specification"))
                .and_then(|wf| wf.get("$graph"))
                .and_then(|g| g.as_array())
                .ok_or_else(|| "Expected graph array".to_string())?;
            let working_dir = metadata.working_dir.clone().unwrap_or_default();
            export_rocrate(
                Some(&"rocrate".to_string()),
                Some(&working_dir),
                &cwl_path.unwrap_or_default(),
                metadata.profile,
                Some("remote"),
                graph_array,
                Some(&logs),
            )
            .await
            .map_err(|e| e.to_string())?
        }
    };
    export_state.set(ExportState::Success);
    Ok(())
}

#[component]
pub fn TerminalViewer(#[props(optional)] exec_type: Option<ExecutionType>) -> Element {
    let app_state = use_app_state();
    let exec_type = exec_type.unwrap_or_else(|| (*app_state().terminal_exec_type.read()).clone());
    let is_workflow = if exec_type == ExecutionType::Local {
        if let Some(exec_file) = (app_state().last_local_execution_file)() {
            read_node_type(&exec_file) == FileType::Workflow
        } else {
            false
        }
    } else {
        false
    };
    let local_cwl = if exec_type == ExecutionType::Local {
        (app_state().last_local_execution_file)()
            .map(|p| p.to_string_lossy().to_string())
    } else {
        None
    };
    let terminal_log = &app_state().terminal_log;
    let toast_items = use_context::<Signal<Vec<ToastItem>>>();
    let mut export_state = use_signal(|| ExportState::Idle);
    let (instance, token) = if exec_type == ExecutionType::Remote {
        match get_reana_credentials().ok().flatten() {
            Some((i, t)) => (Some(i), Some(t)),
            None => (None, None),
        }
    } else {
        (None, None)
    };
    let reana_instance = if exec_type == ExecutionType::Remote {
        match (instance.clone(), token.clone()) {
            (Some(i), Some(t)) => Some(Arc::new(reana::reana::Reana::new(i, t))),
            _ => None,
        }
    } else {
        None
    };
    use_effect(move || {
        let state = export_state();
        if matches!(state, ExportState::Success | ExportState::Error(_)) {
            spawn(async move {
                tokio::time::sleep(Duration::from_secs(3)).await;
                export_state.set(ExportState::Idle);
            });
        }
    });
    let open = use_signal(|| true);
    rsx! {
        div { class: "h-full w-full flex flex-col bg-black text-green-400",
            TerminalHeader { exec_type: exec_type.clone() }
            if exec_type == ExecutionType::Remote {
                RemoteActions {
                    export_state,
                    toast_items,
                    instance: instance.clone().unwrap_or_default(),
                    token: token.clone().unwrap_or_default(),
                }
            } else {
                LocalActions { export_state, toast_items, is_workflow }
            }
            TerminalOutput {
                log: terminal_log()
            }
            if let ExportState::WaitingForMetadata(dialog) = export_state() {
                MetadataDialog {
                    state: dialog.clone(),
                    open,
                    on_submit: {
                        let mut export_state = export_state;
                        let instance = instance.clone();
                        let token = token.clone();
                        let local_cwl = local_cwl.clone();
                        let exec_type = exec_type.clone();
                        let reana_instance = reana_instance.clone();
                        move |mut metadata: MetadataDialogState| {
                            let instance = instance.clone();
                            let token = token.clone();
                            let local_cwl = local_cwl.clone();
                            let exec_type = exec_type.clone();
                            let reana_instance = reana_instance.clone();
                            spawn(async move {
                                let cwl_file = match exec_type {
                                    ExecutionType::Local => local_cwl.clone(),
                                    ExecutionType::Remote => {
                                        let workflow_name = match get_last_workflow_name().await {
                                            Ok(n) => n,
                                            Err(e) => {
                                                export_state.set(ExportState::Error(e.to_string()));
                                                return;
                                            }
                                        };
                                        match get_cwl_name(reana_instance.clone(), &workflow_name).await {
                                            Ok(cwl_name) => {
                                                let path = app_state()
                                                    .working_directory
                                                    .as_ref()
                                                    .map(|dir| dir.join(&cwl_name));

                                                path.map(|p| p.to_string_lossy().to_string())
                                            }
                                            Err(e) => {
                                                export_state.set(ExportState::Error(e.to_string()));
                                                return;
                                            }
                                        }
                                    }
                                };
                                if exec_type == ExecutionType::Local {
                                    metadata.name = cwl_file
                                        .as_ref()
                                        .and_then(|p| std::path::Path::new(p)
                                            .file_stem()
                                            .map(|s| s.to_string_lossy().to_string()));
                                }
                                if let Some(cwl_name) = &cwl_file {
                                    if let Some(name) = &metadata.name
                                        && let Err(e) = annotate_field(cwl_name, "label", name)
                                    {
                                        export_state.set(ExportState::Error(e.to_string()));
                                        return;
                                    }
                                    if let Some(description) = &metadata.description
                                        && let Err(e) = annotate_field(cwl_name, "doc", description)
                                    {
                                        export_state.set(ExportState::Error(e.to_string()));
                                        return;
                                    }
                                    if let Some(license) = &metadata.license
                                        && let Err(e) = annotate_license(cwl_name, &Some(license.clone())).await
                                    {
                                        export_state.set(ExportState::Error(e.to_string()));
                                        return;
                                    }
                                }
                                export_state.set(ExportState::Running);
                                let result = run_export(
                                    metadata.clone(),
                                    export_state,
                                    cwl_file,
                                    instance,
                                    token,
                                    exec_type,
                                ).await;
                                if let Err(e) = result {
                                    export_state.set(ExportState::Error(e));
                                }
                            });
                        }
                    },
                    on_close: Some(EventHandler::new({
                        let mut export_state = export_state;
                        move |_| {
                            export_state.set(ExportState::Idle);
                        }
                    }))
                }
            }
            match export_state() {
                ExportState::Running => rsx! {
                    div { class: "text-blue-400 px-3 py-1", "Exporting..." }
                },
                ExportState::Success => rsx! {
                    div { class: "text-green-400 px-3 py-1", "Export complete!" }
                },
                ExportState::Error(err) => rsx! {
                    div { class: "text-red-400 px-3 py-1", "Error: {err}" }
                },
                _ => rsx! {}
            }
        }
    }
}


#[component]
pub fn GlobalTerminal() -> Element {
    let app_state = use_app_state();
    rsx! {
        div { 
            class: "h-full w-full flex flex-col bg-black text-green-400 p-4",
            TerminalViewer { exec_type: Some((*app_state().terminal_exec_type.read()).clone()) }
        }
    }
}