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

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::super::{WorkflowSlot, add_workflow_output_connection, add_workflow_step_connection};
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn step_link_merge_set_and_clear() {
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

        set_step_input_link_merge_mut(
            &mut wf,
            "consumer",
            "value",
            Some(LinkMergeMethod::MergeFlattened),
        )
        .unwrap();
        assert_eq!(
            wf.get_step("consumer").unwrap().r#in[0].link_merge,
            Some(LinkMergeMethod::MergeFlattened)
        );
        set_step_input_link_merge_mut(&mut wf, "consumer", "value", None).unwrap();
        assert_eq!(wf.get_step("consumer").unwrap().r#in[0].link_merge, None);
    }

    #[test]
    fn output_link_merge_set_and_clear() {
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

        set_output_link_merge_mut(&mut wf, "final", Some(LinkMergeMethod::MergeFlattened)).unwrap();
        let output = wf
            .outputs
            .iter()
            .find(|o| o.id.as_deref() == Some("final"))
            .unwrap();
        assert_eq!(output.link_merge, Some(LinkMergeMethod::MergeFlattened));

        set_output_link_merge_mut(&mut wf, "final", None).unwrap();
        let output = wf
            .outputs
            .iter()
            .find(|o| o.id.as_deref() == Some("final"))
            .unwrap();
        assert_eq!(output.link_merge, None);
    }
}
