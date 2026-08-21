use std::{collections::HashMap, path::PathBuf};

use bon::Builder;
use chrono::Utc;
use commonwl::packed::PackedCWL;
use rocrate::{context::Context, profile::Profile};

use crate::project::config::WorkflowConfig;

#[derive(Debug, Clone, Default)]
pub struct Engine {
    pub name: String,
    pub version: Option<String>,
}

/// Everything [`crate::provenance::builder::build_crate`] needs, gathered from wherever the
/// caller's backend keeps it.
#[derive(Debug, Clone, Builder)]
pub struct CrateInputs {
    pub workflow: PackedCWL,
    /// How `workflow` is represented as crate payload, see [`WorkflowLayout`]. Defaults to a
    /// single `"workflow.json"`, matching REANA (which only ever hands back one packed file).
    #[builder(default = WorkflowLayout::Packed { file_name: "workflow.json".to_string() })]
    pub layout: WorkflowLayout,
    pub metadata: WorkflowConfig,
    pub run: RunRecord,
    #[builder(default = default_profiles())]
    pub profiles: Vec<Profile>,
    #[builder(default = Context::ro_crate_1_1())]
    pub context: Context,
    pub date_published: chrono::DateTime<Utc>,
    #[builder(default)]
    pub payload: Vec<PayloadFile>,
}

/// How the packed workflow is represented as crate payload.
///
/// Every entity id [`crate::provenance::builder::build_crate`] derives from the packed graph
/// (e.g. `"#main/population"`) gets turned into a crate-relative id through whichever variant is
/// in play -- see [`WorkflowLayout::prefixed`].
#[derive(Debug, Clone)]
pub enum WorkflowLayout {
    /// One JSON file holds the whole packed `$graph`. Simple, self-contained, but not directly
    /// runnable without unpacking it again -- what REANA always gets, since it only ever hands
    /// back one packed file.
    Packed { file_name: String },
    /// The original per-file CWL project, still directly re-executable with any CWL runner.
    /// Keyed by each document's own packed-graph id (e.g. `"#main"`, `"#calculation.cwl"`) to
    /// the crate-relative name it's written under (e.g. `"main.cwl"`, `"calculation.cwl"`).
    /// Only available where the original files are actually on disk -- local execution, not
    /// REANA.
    Files { file_names: HashMap<String, String> },
}

impl WorkflowLayout {
    /// The crate-relative file `id` (a packed-graph id) belongs to, with no fragment --
    /// `Packed`'s one file for anything, or whichever document's own id prefixes `id` for
    /// `Files`. Empty if `Files` doesn't have an entry covering `id` -- callers only hit this on
    /// ids `build_crate` derived from the same graph `file_names` was built from, so it
    /// shouldn't happen in practice.
    #[must_use]
    pub fn owning_file(&self, id: &str) -> &str {
        match self {
            WorkflowLayout::Packed { file_name } => file_name,
            WorkflowLayout::Files { file_names } => self
                .owner(file_names, id)
                .map_or("", |(_, file_name)| file_name),
        }
    }

    /// `id`, scoped to this layout: the whole packed id appended to the one file for `Packed`
    /// (`"workflow.json#calculation.cwl/population"`), or just the part local to whichever file
    /// actually owns it for `Files` (`"calculation.cwl#population"`) -- repeating the owning
    /// document's own id inside its own file would be redundant once it has one.
    #[must_use]
    pub fn prefixed(&self, id: &str) -> String {
        match self {
            WorkflowLayout::Packed { file_name } => format!("{file_name}{id}"),
            WorkflowLayout::Files { file_names } => match self.owner(file_names, id) {
                Some((doc_id, file_name)) if id == doc_id => file_name.to_string(),
                Some((doc_id, file_name)) => format!("{file_name}#{}", &id[doc_id.len() + 1..]),
                None => id.to_string(),
            },
        }
    }

    #[must_use]
    pub fn is_files(&self) -> bool {
        matches!(self, WorkflowLayout::Files { .. })
    }

    fn owner<'a>(
        &self,
        file_names: &'a HashMap<String, String>,
        id: &str,
    ) -> Option<(&'a str, &'a str)> {
        file_names
            .iter()
            .filter(|(doc_id, _)| id == doc_id.as_str() || id.starts_with(&format!("{doc_id}/")))
            .max_by_key(|(doc_id, _)| doc_id.len())
            .map(|(doc_id, file_name)| (doc_id.as_str(), file_name.as_str()))
    }
}

/// Process, Workflow and Provenance Run Crate, plus Workflow RO-Crate -- the profiles a REANA
/// export conforms to.
#[must_use]
pub fn default_profiles() -> Vec<Profile> {
    vec![
        Profile::ProcessRun("0.5".to_string()),
        Profile::WorkflowRun("0.5".to_string()),
        Profile::ProvenanceRun("0.5".to_string()),
        Profile::WorkflowRoCrate("1.0".to_string()),
    ]
}

/// A file the crate carries alongside its metadata: a workflow input, output or intermediate
/// result. `size`/`checksum` are best-effort -- left unset when the backend does not have them.
#[derive(Debug, Clone)]
pub struct PayloadFile {
    /// Crate-relative name, e.g. `"population.csv"`.
    pub name: String,
    pub size: Option<u64>,
    /// Hex SHA-1 digest, stored under the crate's `sha1` term.
    pub checksum: Option<String>,
    pub source: Option<PayloadSource>,
}

#[derive(Debug, Clone)]
pub enum PayloadSource {
    Local(PathBuf),
    Remote(String),
}

#[derive(Debug, Clone, Default)]
pub struct RunRecord {
    pub engine: Engine,
    pub started_at: Option<chrono::DateTime<Utc>>,
    pub ended_at: Option<chrono::DateTime<Utc>>,
    pub steps: HashMap<String, StepRun>, // keyed by packed step id, "#main/calculation"
}

#[derive(Debug, Clone, Default)]
pub struct StepRun {
    pub started_at: Option<chrono::DateTime<Utc>>,
    pub ended_at: Option<chrono::DateTime<Utc>>,
    pub container_image: Option<String>, // as pulled: "pandas/pandas:pip-all"
}
