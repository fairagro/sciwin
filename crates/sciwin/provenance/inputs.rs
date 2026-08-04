use std::collections::HashMap;

use chrono::Utc;

#[derive(Debug, Clone, Default)]
pub struct RunRecord {
    pub engine: Engine, // name + optional version
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
