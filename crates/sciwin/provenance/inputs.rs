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
/// caller's backend keeps it. Building the `RoCrate` itself touches neither a clock nor the
/// filesystem -- `date_published` is injected here instead of read from `Utc::now()`, which is
/// what makes `build_crate` deterministic and its output byte-comparable in tests.
#[derive(Debug, Clone, Builder)]
pub struct CrateInputs {
    pub workflow: PackedCWL,
    /// Crate-relative name the packed workflow is written under, and the prefix every entity id
    /// derived from the packed graph carries (e.g. `"workflow.json#main/population"`).
    #[builder(default = "workflow.json".to_string(), into)]
    pub workflow_file: String,
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
