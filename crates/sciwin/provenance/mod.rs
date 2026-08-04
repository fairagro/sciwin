pub mod reana;
pub mod task;

/// Result alias for this module. See [`crate::Result`] for code spanning several modules.
pub type Provenance<T> = Result<T, ProvenanceError>;

/// Anything project initialization can fail with.
#[derive(Error, Diagnostic, Debug)]
pub enum ProvenanceError {}
