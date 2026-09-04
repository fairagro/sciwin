use crate::authoring::{AuthoringError, AuthoringResult};
use commonwl::{documents::Workflow, outputs::LinkMergeMethod};

/// Sets or clears the `linkMerge` method on `step_id`'s `port` input.
/// # Errors
/// If no step with `step_id`, or no input `port` on it, exists
pub fn set_step_input_link_merge_mut(
    workflow: &mut Workflow,
    step_id: &str,
    port: &str,
    method: Option<LinkMergeMethod>,
) -> AuthoringResult<()> {
    let step = workflow
        .steps
        .iter_mut()
        .find(|s| s.id.as_deref() == Some(step_id))
        .ok_or_else(|| AuthoringError::InvalidWorkflowStep {
            id: step_id.to_string(),
        })?;
    let input = step
        .r#in
        .iter_mut()
        .find(|i| i.id.as_deref() == Some(port))
        .ok_or_else(|| AuthoringError::InvalidWorkflowInput {
            id: port.to_string(),
            path: format!("step {step_id}"),
        })?;
    input.link_merge = method;
    Ok(())
}

/// Sets or clears `linkMerge` on a workflow output.
/// # Errors
/// If no output with `output_id` exists
pub fn set_output_link_merge_mut(
    workflow: &mut Workflow,
    output_id: &str,
    method: Option<LinkMergeMethod>,
) -> AuthoringResult<()> {
    let output = workflow
        .outputs
        .iter_mut()
        .find(|o| o.id.as_deref() == Some(output_id))
        .ok_or_else(|| AuthoringError::InvalidWorkflowOutput {
            id: output_id.to_string(),
            path: "workflow".to_string(),
        })?;
    output.link_merge = method;
    Ok(())
}
