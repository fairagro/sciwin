use crate::authoring::{AuthoringError, AuthoringResult};
use commonwl::documents::Workflow;

/// Sets or clears `step_id`'s `when:` guard expression.
/// # Errors
/// If no step with `step_id` exists
pub fn set_step_when_mut(
    workflow: &mut Workflow,
    step_id: &str,
    expression: Option<String>,
) -> AuthoringResult<()> {
    let step = workflow
        .steps
        .iter_mut()
        .find(|s| s.id.as_deref() == Some(step_id))
        .ok_or_else(|| AuthoringError::InvalidWorkflowStep {
            id: step_id.to_string(),
        })?;
    step.when = expression;
    Ok(())
}
