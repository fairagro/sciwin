use crate::{
    components::files::{FileType, read_node_type},
    workflow::VisualWorkflow,
};
use dioxus::{html::geometry::ClientPoint, prelude::*, router::RouterContext};
use pathdiff::diff_paths;
use petgraph::graph::NodeIndex;
use repository::{Repository, commit, stage_file};
use s4n_core::{config::Config, project::initialize_project};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    env::temp_dir,
    fs,
    path::{Path, PathBuf},
    time::Duration,
};
use crate::components::ExecutionType;

pub mod components;
pub mod files;
pub mod graph;
pub mod layout;
pub mod types;
pub mod workflow;
pub mod reana_integration;

#[derive(Default, Clone, Debug, Serialize, Deserialize)]
pub struct ApplicationState {
    pub working_directory: Option<PathBuf>,
    pub current_file: Option<PathBuf>,
    #[serde(skip)]
    pub config: Option<Config>,
    #[serde(skip)]
    pub workflow: VisualWorkflow,
    #[serde(skip)]
    data_transfer: serde_json::Value,
    #[serde(skip)]
    pub active_tab: Signal<String>,
    #[serde(skip)]
    pub show_terminal_log: Signal<bool>,
    #[serde(skip)]
    pub terminal_log: Signal<String>,
    #[serde(skip)]
    pub terminal_exec_type: Signal<ExecutionType>,
    #[serde(skip)]
    pub show_manage_reana_modal: Signal<bool>,
}

impl ApplicationState {
    pub fn set_data_transfer<T: Serialize>(&mut self, item: &T) -> anyhow::Result<()> {
        let value = serde_json::to_value(item)?;
        self.data_transfer = value;
        Ok(())
    }

    pub fn get_data_transfer<T: DeserializeOwned>(&self) -> anyhow::Result<T> {
        let item = serde_json::from_value(self.data_transfer.clone())?;
        Ok(item)
    }
}

#[derive(Default, Debug, Clone)]
pub enum DragState {
    #[default]
    None, // not used maybe
    Node(NodeIndex), //used when drag starts on Node Header
    Connection {
        //used when drag starts from slot
        source_node: NodeIndex,
        source_port: String,
    },
}

#[derive(Default, Clone, Debug)]
pub struct DragContext {
    pub dragging: Option<DragState>,
    pub drag_offset: Signal<ClientPoint>,
}

//used to open a project
#[derive(Default, Clone, Debug)]
pub struct ProjectInfo {
    pub working_directory: PathBuf,
    pub config: Config,
}

pub fn use_app_state() -> Signal<ApplicationState> {
    use_context::<Signal<ApplicationState>>()
}

pub fn use_drag() -> Signal<DragContext> {
    use_context::<Signal<DragContext>>()
}
pub async fn open_project(path: impl AsRef<Path>, mut open: Signal<bool>, mut confirmed: Signal<bool>) -> anyhow::Result<Option<ProjectInfo>> {
    let config_path = path.as_ref().join("workflow.toml");

    if !config_path.exists() {
        open.set(true);

        {
            let path = path.as_ref().to_owned();
            // Check dialog result
            loop {
                if !open() {
                    if confirmed() {
                        initialize_project(&path, false).map_err(|e| anyhow::anyhow!("{e}"))?;
                        confirmed.set(false); //reset
                        return Ok::<_, anyhow::Error>(Some(open_project_inner(path.as_ref())?));
                    }
                    return Ok(None);
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    } else {
        Ok(Some(open_project_inner(path.as_ref())?))
    }
}

fn open_project_inner(path: &Path) -> anyhow::Result<ProjectInfo> {
    let config_path = path.join("workflow.toml");
    let toml = std::fs::read_to_string(config_path)?;
    let config: Config = toml::from_str(&toml)?;
    Ok(ProjectInfo {
        working_directory: path.to_path_buf(),
        config,
    })
}

fn open_file(path: impl AsRef<Path>, router: RouterContext) {
    if path.as_ref().exists() {
        match read_node_type(&path) {
            FileType::Workflow => router.push(format!("/workflow?path={}", path.as_ref().to_string_lossy())),
            FileType::Other => router.push("/"),
            _ => router.push(format!("/tool?path={}", path.as_ref().to_string_lossy())),
        };
    }
}

pub fn last_session_data() -> PathBuf {
    let tmp = temp_dir().join("s4n");

    if !tmp.exists() {
        fs::create_dir_all(&tmp).expect("Could not create temp directory");
    }

    tmp.join("app_state.json")
}

pub async fn restore_last_session(open: Signal<bool>, confirmed: Signal<bool>) -> anyhow::Result<Option<ApplicationState>> {
    if last_session_data().exists() {
        let data = fs::read_to_string(last_session_data())?;
        let mut state: ApplicationState = serde_json::from_str(&data)?;

        if let Some(working_dir) = &state.working_directory {
            let info = open_project(working_dir, open, confirmed).await?;
            if let Some(info) = info {
                state.working_directory = Some(info.working_directory);
                state.config = Some(info.config);
            }
        }

        if let Some(current_file) = &state.current_file
            && current_file.exists()
        {
            open_file(current_file, router());
        }
        Ok(Some(state))
    } else {
        Ok(None)
    }
}

//Saves AND commits a file
pub fn save_file(working_dir: &Path, filename: impl AsRef<Path>, message: &str) -> anyhow::Result<()> {
    let repo = Repository::open(working_dir)?;
    stage_file(&repo, filename)?;
    commit(&repo, message)?;

    Ok(())
}

//Saves AND commits a file
pub fn save_file_canonical(filename: impl AsRef<Path>, message: &str) -> anyhow::Result<()> {
    let mut residual = filename.as_ref();
    let mut relative_filename = None;
    loop {
        if Repository::open(residual).is_ok()
            && let Some(relative_filename) = relative_filename
        {
            save_file(residual, relative_filename, message)?;
            break;
        } else {
            residual = residual.parent().unwrap_or(Path::new("/"));
            if residual == Path::new("/") {
                break; //can't get down any further
            }
            relative_filename = diff_paths(filename.as_ref(), residual);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::env;

    use super::*;
    use fstest::fstest;

    #[fstest(repo = true)]
    fn test_save_file_canonical() {
        fs::create_dir("./test_dir").unwrap();
        fs::write("./test_dir/161.txt", "haha sciwin go brr").unwrap();

        let filename = env::current_dir().unwrap().join("./test_dir/161.txt");
        let repo = Repository::open_from_env().unwrap();
        let modified_files = repository::get_modified_files(&repo);
        assert_eq!(modified_files.len(), 1);

        assert!(save_file_canonical(filename, "haha sciwin go brr").is_ok());

        let modified_files = repository::get_modified_files(&repo);
        assert_eq!(modified_files.len(), 0);
    }
}
