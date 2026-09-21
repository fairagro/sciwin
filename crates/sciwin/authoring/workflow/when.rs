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

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::super::add_workflow_step;
    use super::*;
    use commonwl::load_cwl_file;
    use tempfile::tempdir;

    #[test]
    fn set_and_clear_when() {
        let dir = tempdir().unwrap();
        let tool_path = dir.path().join("tool.cwl");
        write_tool(&tool_path, "in", "out");
        let mut wf = Workflow::default();
        add_workflow_step(
            &mut wf,
            dir.path().join("workflow.cwl"),
            "step",
            &tool_path,
            &load_cwl_file(&tool_path, true).unwrap(),
        )
        .unwrap();

        set_step_when_mut(&mut wf, "step", Some("$(inputs.x != null)".to_string())).unwrap();
        assert_eq!(
            wf.get_step("step").unwrap().when.as_deref(),
            Some("$(inputs.x != null)")
        );

        set_step_when_mut(&mut wf, "step", None).unwrap();
        assert_eq!(wf.get_step("step").unwrap().when, None);
    }
}
