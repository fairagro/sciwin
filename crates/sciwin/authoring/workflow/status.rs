use crate::authoring::{AuthoringError, AuthoringResult};
use commonwl::{
    documents::{CWLDocument, StringOrDocument, Workflow},
    load_cwl_file,
};
use serde::Serialize;
use std::path::Path;

/// How a single input or output slot is wired.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SlotStatus {
    /// Inputs: something feeds it. Outputs: something consumes it.
    Connected,
    /// Not connected, but a default value covers it. Inputs only.
    Default,
    Unconnected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SlotState {
    pub id: String,
    pub status: SlotStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StepState {
    pub id: String,
    /// The step's `run` path, `None` for an inline document.
    pub run: Option<String>,
    pub inputs: Vec<SlotState>,
    pub outputs: Vec<SlotState>,
}

/// Wiring of a workflow's own inputs and outputs, and of every step's slots.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConnectionStatus {
    pub inputs: Vec<SlotState>,
    pub outputs: Vec<SlotState>,
    pub steps: Vec<StepState>,
}

/// Computes the [`ConnectionStatus`] of `workflow`. Step `run` paths resolve against
/// `workflow_path`'s directory.
pub fn connection_status(
    workflow: &Workflow,
    workflow_path: impl AsRef<Path>,
) -> AuthoringResult<ConnectionStatus> {
    let base = workflow_path.as_ref().parent().unwrap_or(Path::new("."));

    // every source a step input or workflow output reads from
    let sources: Vec<&str> = workflow
        .steps
        .iter()
        .flat_map(|step| &step.r#in)
        .filter_map(|input| input.source.as_ref())
        .chain(
            workflow
                .outputs
                .iter()
                .filter_map(|o| o.output_source.as_ref()),
        )
        .flatten()
        .map(|s| s.trim_start_matches('#'))
        .collect();
    let is_read = |id: &str| sources.contains(&id);

    let inputs = workflow
        .inputs
        .iter()
        .filter_map(|input| {
            let id = input.id.clone()?;
            let status = input_status(is_read(&id), input.default.is_some());
            Some(SlotState { id, status })
        })
        .collect();

    let mut step_outputs = vec![];
    let mut steps = vec![];
    for step in &workflow.steps {
        let step_id = step.id.clone().unwrap_or_default();
        let (doc, run) = match &step.run {
            StringOrDocument::String(run) => {
                let path = base.join(run);
                let doc = load_cwl_file(&path, true).map_err(|e| {
                    AuthoringError::Unknown(anyhow::anyhow!(
                        "could not load step {step_id} from {}: {e}",
                        path.display()
                    ))
                })?;
                (doc, Some(run.clone()))
            }
            StringOrDocument::Document(doc) => (CWLDocument::clone(doc), None),
        };

        let inputs = doc
            .get_inputs()
            .into_iter()
            .filter_map(|input| {
                let id = input.id?;
                let wired = step.r#in.iter().any(|i| i.id.as_deref() == Some(&id));
                let status = input_status(wired, input.default.is_some());
                Some(SlotState { id, status })
            })
            .collect();
        let outputs = doc
            .get_output_ids()
            .into_iter()
            .map(|id| {
                let target = format!("{step_id}/{id}");
                step_outputs.push(target.clone());
                let status = if is_read(&target) {
                    SlotStatus::Connected
                } else {
                    SlotStatus::Unconnected
                };
                SlotState { id, status }
            })
            .collect();
        steps.push(StepState {
            id: step_id,
            run,
            inputs,
            outputs,
        });
    }

    let outputs = workflow
        .outputs
        .iter()
        .map(|output| {
            let fed = output.output_source.iter().flatten().any(|src| {
                let src = src.trim_start_matches('#');
                step_outputs.iter().any(|o| o == src)
                    || workflow.inputs.iter().any(|i| i.id.as_deref() == Some(src))
            });
            SlotState {
                id: output.id.clone().unwrap_or_default(),
                status: if fed {
                    SlotStatus::Connected
                } else {
                    SlotStatus::Unconnected
                },
            }
        })
        .collect();

    Ok(ConnectionStatus {
        inputs,
        outputs,
        steps,
    })
}

fn input_status(connected: bool, has_default: bool) -> SlotStatus {
    match (connected, has_default) {
        (true, _) => SlotStatus::Connected,
        (false, true) => SlotStatus::Default,
        (false, false) => SlotStatus::Unconnected,
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::write_tool;
    use super::super::{
        WorkflowSlot, add_workflow_input_connection, add_workflow_output_connection,
        add_workflow_step_connection,
    };
    use super::*;
    use commonwl::{
        OneOrMany,
        inputs::{DefaultValue, InputType},
        types::CWLType,
    };
    use std::fs;
    use tempfile::tempdir;

    fn status_of<'a>(slots: &'a [SlotState], id: &str) -> &'a SlotStatus {
        &slots.iter().find(|s| s.id == id).unwrap().status
    }

    #[test]
    fn workflow_input_with_different_name_counts_as_connected() {
        let dir = tempdir().unwrap();
        let tool = dir.path().join("calc.cwl");
        let wf_path = dir.path().join("wf.cwl");
        write_tool(&tool, "population", "results");

        let mut wf = Workflow::default();
        add_workflow_input_connection(
            &mut wf,
            &wf_path,
            "data",
            WorkflowSlot::new(&tool, "calc", "population"),
        )
        .unwrap();
        wf.add_workflow_input_mut(
            "threshold",
            OneOrMany::One(InputType::CWLType(CWLType::Int)),
            Some(DefaultValue::Any(3.into())),
        );
        wf.add_workflow_input_mut(
            "unused",
            OneOrMany::One(InputType::CWLType(CWLType::Int)),
            None,
        );

        let status = connection_status(&wf, &wf_path).unwrap();
        assert_eq!(status_of(&status.inputs, "data"), &SlotStatus::Connected);
        assert_eq!(status_of(&status.inputs, "threshold"), &SlotStatus::Default);
        assert_eq!(
            status_of(&status.inputs, "unused"),
            &SlotStatus::Unconnected
        );
        let step = &status.steps[0];
        assert_eq!(step.id, "calc");
        assert_eq!(step.run.as_deref(), Some("calc.cwl"));
        assert_eq!(
            status_of(&step.inputs, "population"),
            &SlotStatus::Connected
        );
        assert_eq!(
            status_of(&step.outputs, "results"),
            &SlotStatus::Unconnected
        );
    }

    #[test]
    fn reports_step_to_step_and_output_links() {
        let dir = tempdir().unwrap();
        let calc = dir.path().join("calc.cwl");
        let plot = dir.path().join("plot.cwl");
        let wf_path = dir.path().join("wf.cwl");
        write_tool(&calc, "population", "results");
        write_tool(&plot, "results", "image");

        let mut wf = Workflow::default();
        add_workflow_step_connection(
            &mut wf,
            &wf_path,
            WorkflowSlot::new(&calc, "calc", "results"),
            WorkflowSlot::new(&plot, "plot", "results"),
        )
        .unwrap();
        add_workflow_output_connection(
            &mut wf,
            &wf_path,
            WorkflowSlot::new(&plot, "plot", "image"),
            "out",
        )
        .unwrap();

        let status = connection_status(&wf, &wf_path).unwrap();
        assert_eq!(status_of(&status.outputs, "out"), &SlotStatus::Connected);
        let calc_step = status.steps.iter().find(|s| s.id == "calc").unwrap();
        assert_eq!(
            status_of(&calc_step.inputs, "population"),
            &SlotStatus::Unconnected
        );
        assert_eq!(
            status_of(&calc_step.outputs, "results"),
            &SlotStatus::Connected
        );
    }

    #[test]
    fn missing_step_file_is_an_error() {
        let dir = tempdir().unwrap();
        let tool = dir.path().join("calc.cwl");
        let wf_path = dir.path().join("wf.cwl");
        write_tool(&tool, "population", "results");

        let mut wf = Workflow::default();
        add_workflow_input_connection(
            &mut wf,
            &wf_path,
            "population",
            WorkflowSlot::new(&tool, "calc", "population"),
        )
        .unwrap();
        fs::remove_file(&tool).unwrap();

        let err = connection_status(&wf, &wf_path).unwrap_err().to_string();
        assert!(err.contains("could not load step calc"), "{err}");
    }
}
