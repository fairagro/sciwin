use crate::authoring::{AuthoringError, AuthoringResult};
use commonwl::{documents::Workflow, outputs::PickValueMethod};

/// Sets the `pickValue` resolution strategy on `step_id`'s `port` input,
/// used when it has more than one source feeding it.
/// # Errors
/// If no step with `step_id`, or no input `port` on it, exists
pub fn set_step_pick_value_mut(
    workflow: &mut Workflow,
    step_id: &str,
    port: &str,
    method: PickValueMethod,
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
    input.pick_value = Some(method);
    Ok(())
}

/// Clears a `pickValue` strategy on `step_id`'s `port` input.
/// # Errors
/// If no step with `step_id`, or no input `port` on it, exists
pub fn clear_step_pick_value_mut(
    workflow: &mut Workflow,
    step_id: &str,
    port: &str,
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
    input.pick_value = None;
    Ok(())
}

/// Sets or clears `pickValue` on a workflow output.
/// # Errors
/// If no output with `output_id` exists
pub fn set_output_pick_value_mut(
    workflow: &mut Workflow,
    output_id: &str,
    method: Option<PickValueMethod>,
) -> AuthoringResult<()> {
    let output = workflow
        .outputs
        .iter_mut()
        .find(|o| o.id.as_deref() == Some(output_id))
        .ok_or_else(|| AuthoringError::InvalidWorkflowOutput {
            id: output_id.to_string(),
            path: "workflow".to_string(),
        })?;
    output.pick_value = method;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::super::{WorkflowSlot, add_workflow_output_connection, add_workflow_step_connection};
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn step_pick_value_set_and_clear() {
        let dir = tempdir().unwrap();
        let producer = dir.path().join("producer.cwl");
        let consumer = dir.path().join("consumer.cwl");
        let workflow_path = dir.path().join("workflow.cwl");
        write_tool(&producer, "dummy", "value");
        write_tool(&consumer, "value", "result");

        let mut wf = Workflow::default();
        add_workflow_step_connection(
            &mut wf,
            &workflow_path,
            WorkflowSlot::new(&producer, "producer", "value"),
            WorkflowSlot::new(&consumer, "consumer", "value"),
        )
        .unwrap();

        set_step_pick_value_mut(&mut wf, "consumer", "value", PickValueMethod::AllNonNull).unwrap();
        assert_eq!(
            wf.get_step("consumer").unwrap().r#in[0].pick_value,
            Some(PickValueMethod::AllNonNull)
        );
        clear_step_pick_value_mut(&mut wf, "consumer", "value").unwrap();
        assert_eq!(wf.get_step("consumer").unwrap().r#in[0].pick_value, None);
    }

    #[test]
    fn output_pick_value_set_and_clear() {
        let dir = tempdir().unwrap();
        let tool_path = dir.path().join("tool.cwl");
        write_tool(&tool_path, "in", "out");
        let mut wf = Workflow::default();
        add_workflow_output_connection(
            &mut wf,
            dir.path().join("workflow.cwl"),
            WorkflowSlot::new(&tool_path, "tool", "out"),
            "final",
        )
        .unwrap();

        set_output_pick_value_mut(&mut wf, "final", Some(PickValueMethod::FirstNonNull)).unwrap();
        let output = wf
            .outputs
            .iter()
            .find(|o| o.id.as_deref() == Some("final"))
            .unwrap();
        assert_eq!(output.pick_value, Some(PickValueMethod::FirstNonNull));

        set_output_pick_value_mut(&mut wf, "final", None).unwrap();
        let output = wf
            .outputs
            .iter()
            .find(|o| o.id.as_deref() == Some("final"))
            .unwrap();
        assert_eq!(output.pick_value, None);
    }
}
