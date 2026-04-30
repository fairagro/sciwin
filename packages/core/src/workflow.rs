use anyhow::{Context, Result};
use commonwl::{
    OneOrMany,
    documents::{CWLDocument, StringOrDocument, Workflow, WorkflowStep},
    format::format_cwl,
    inputs::{WorkflowInputParameter, WorkflowStepInput},
    load_cwl_file,
    outputs::{StringOrWorkflowStepOutput, WorkflowOutputParameter},
    requirements::{SubworkflowFeatureRequirement, WorkflowRequirements},
};
use std::{fs, path::Path};

pub fn create_workflow(filename: impl AsRef<Path>, force: bool) -> Result<String> {
    let wf = CWLDocument::Workflow(Workflow::default());
    let filename = filename.as_ref();

    let mut yaml = serde_yaml::to_string(&wf)?;
    yaml = format_cwl(&yaml).map_err(|e| anyhow::anyhow!("Could not formal yaml: {e}"))?;

    //removes file first if exists and force is given
    if force && filename.exists() {
        fs::remove_file(filename)?;
    }

    let name = Path::new(&filename)
        .file_stem()
        .and_then(|s| s.to_str())
        .context("Could not determine workflow name from filename")?;

    let parent = filename
        .parent()
        .context("Could not determine parent directory of workflow file")?;
    fs::create_dir_all(parent).with_context(|| {
        format!(
            "Could not create parent directory for workflow file at {}",
            parent.to_string_lossy()
        )
    })?;
    fs::write(filename, &yaml).map_err(|e| {
        anyhow::anyhow!(
            "❌ Could not create workflow {} at {}: {}",
            name,
            filename.to_string_lossy(),
            e
        )
    })?;
    Ok(yaml)
}

pub fn add_workflow_step(
    workflow: &mut Workflow,
    name: &str,
    path: impl AsRef<Path>,
    doc: &CWLDocument,
) {
    let path = path.as_ref().to_string_lossy().into_owned();
    if !workflow.has_step(name) {
        let path = if path.starts_with("workflows") {
            path.replace("workflows", "..")
        } else {
            format!("../../{path}")
        };
        let workflow_step = WorkflowStep::builder()
            .id(name.to_string())
            .run(StringOrDocument::String(path))
            .out(
                doc.get_output_ids()
                    .iter()
                    .map(|id| StringOrWorkflowStepOutput::String(id.clone()))
                    .collect::<Vec<_>>(),
            )
            .r#in(vec![])
            .build();
        workflow.steps.push(workflow_step);
        if !workflow.has_requirement::<SubworkflowFeatureRequirement>() {
            if let Some(requirements) = &mut workflow.requirements {
                requirements.push(WorkflowRequirements::SubworkflowFeatureRequirement(
                    SubworkflowFeatureRequirement {},
                ));
            } else {
                workflow.requirements =
                    Some(vec![WorkflowRequirements::SubworkflowFeatureRequirement(
                        SubworkflowFeatureRequirement {},
                    )]);
            }
        }
    }
}

/// Adds a connection between an input and a `CommandLineTool`. The tool will be registered as step if it is not already and an Workflow input will be added.
pub fn add_workflow_input_connection(
    workflow: &mut Workflow,
    from_input: &str,
    to_filename: impl AsRef<Path>,
    to_name: &str,
    to_slot_id: &str,
) -> Result<()> {
    let to_filename = to_filename.as_ref();

    let to_cwl = load_cwl_file(to_filename, true)
        .map_err(|e| anyhow::anyhow!("Failed to load CWL document: {e}"))?;
    let to_inputs = to_cwl.get_inputs();
    let to_slot = to_inputs
        .iter()
        .find(|i| i.id == Some(to_slot_id.to_owned()))
        .expect("No slot");

    //register input
    if !workflow.has_input(from_input) {
        let mut input = WorkflowInputParameter::builder()
            .id(from_input)
            .r#type(to_slot.r#type.clone())
            .build();
        input.default = to_slot.default.clone();
        workflow.inputs.push(input);
    }

    add_workflow_step(workflow, to_name, to_filename, &to_cwl);
    //add input in step
    workflow
        .steps
        .iter_mut()
        .find(|step| step.id == Some(to_name.to_owned()))
        .unwrap()
        .r#in
        .push(
            WorkflowStepInput::builder()
                .id(to_slot_id.to_string())
                .source(OneOrMany::One(from_input.to_owned()))
                .build(),
        );
    Ok(())
}

/// Adds a connection between an output and a `CommandLineTool`. The tool will be registered as step if it is not already and an Workflow output will be added.
pub fn add_workflow_output_connection(
    workflow: &mut Workflow,
    from_name: &str,
    from_slot_id: &str,
    from_filename: impl AsRef<Path>,
    to_output: &str,
) -> Result<()> {
    let from_filename = from_filename.as_ref();

    let from_cwl = load_cwl_file(from_filename, true)
        .map_err(|e| anyhow::anyhow!("Failed to load CWL document: {e}"))?;
    let from_type = match &from_cwl {
        CWLDocument::CommandLineTool(clt) => clt
            .outputs
            .iter()
            .find(|i| i.id == Some(from_slot_id.to_owned()))
            .map(|i| i.r#type.clone()),
        CWLDocument::ExpressionTool(et) => et
            .outputs
            .iter()
            .find(|i| i.id == Some(from_slot_id.to_owned()))
            .map(|i| i.r#type.clone().into()),
        CWLDocument::Operation(op) => op
            .outputs
            .iter()
            .find(|i| i.id == Some(from_slot_id.to_owned()))
            .map(|i| i.r#type.clone().into()),
        CWLDocument::Workflow(wf) => wf
            .outputs
            .iter()
            .find(|i| i.id == Some(from_slot_id.to_owned()))
            .map(|i| i.r#type.clone()),
    }
    .expect("No slot");
    add_workflow_step(workflow, from_name, from_filename, &from_cwl);

    if !workflow.has_output(to_output) {
        workflow.outputs.push(
            WorkflowOutputParameter::builder()
                .id(to_output)
                .r#type(from_type)
                .output_source(OneOrMany::One(format!("{from_name}/{from_slot_id}")))
                .build(),
        );
    } else {
        let output = workflow
            .outputs
            .iter_mut()
            .find(|o| o.id == Some(to_output.to_owned()))
            .unwrap();
        output.r#type = from_type;
        output.output_source = Some(OneOrMany::One(format!("{from_name}/{from_slot_id}")));
    }

    Ok(())
}

/// Adds a connection between two `CommandLineTools`. The tools will be registered as step if registered not already.
pub fn add_workflow_step_connection(
    workflow: &mut Workflow,
    from_filename: impl AsRef<Path>,
    from_name: &str,
    from_slot_id: &str,
    to_filename: impl AsRef<Path>,
    to_name: &str,
    to_slot_id: &str,
) -> Result<()> {
    //check if step already exists and create if not
    let from_filename = from_filename.as_ref();
    let to_filename = to_filename.as_ref();

    if !workflow.has_step(from_name) {
        let from_cwl = load_cwl_file(from_filename, true)
            .map_err(|e| anyhow::anyhow!("Failed to load CWL document: {e}"))?;
        let from_outputs = from_cwl.get_output_ids();
        if !from_outputs.contains(&from_slot_id.to_string()) {
            anyhow::bail!(
                "Tool {} does not have output `{}`. Cannot not create node from {:?} in Workflow!",
                from_name,
                from_slot_id,
                from_filename
            );
        }

        //create step
        add_workflow_step(workflow, from_name, from_filename, &from_cwl);
    }

    //check if step exists
    if !workflow.has_step(to_name) {
        let to_cwl = load_cwl_file(to_filename, true)
            .map_err(|e| anyhow::anyhow!("Failed to load CWL document: {e}"))?;
        add_workflow_step(workflow, to_name, to_filename, &to_cwl);
    }

    let step = workflow
        .steps
        .iter_mut()
        .find(|s| s.id == Some(to_name.to_owned()))
        .unwrap(); //safe here!
    step.r#in.push(
        WorkflowStepInput::builder()
            .id(to_slot_id.to_string())
            .source(OneOrMany::One(format!("{from_name}/{from_slot_id}")))
            .build(),
    );

    Ok(())
}

/// Removes a connection between two `CommandLineTools` by removing input from `tool_y` that is also output of `tool_x`.
pub fn remove_workflow_step_connection(
    workflow: &mut Workflow,
    to_name: &str,
    to_slot_id: &str,
) -> Result<()> {
    let step = workflow
        .steps
        .iter_mut()
        .find(|s| s.id == Some(to_name.to_owned()));
    // If the step is found, try to remove the connection by removing input from `tool_y` that uses output of `tool_x`
    // Input is empty, change that?
    if let Some(step) = step {
        if step
            .r#in
            .iter()
            .any(|v| v.id == Some(to_slot_id.to_owned()))
        {
            step.r#in.retain(|v| v.id != Some(to_slot_id.to_owned()));
        }
        Ok(())
    } else {
        anyhow::bail!("Failed to find step {} in workflow!", to_name);
    }
}

/// Removes an input from inputs and removes it from `CommandLineTool` input.
pub fn remove_workflow_input_connection(
    workflow: &mut Workflow,
    from_input: &str,
    to_name: &str,
    to_slot_id: &str,
    remove_input: bool,
) -> Result<()> {
    if remove_input
        && let Some(index) = workflow
            .inputs
            .iter()
            .position(|s| s.id == Some(from_input.to_string()))
    {
        workflow.inputs.remove(index);
    }
    if let Some(step) = workflow
        .steps
        .iter_mut()
        .find(|s| s.id == Some(to_name.to_owned()))
    {
        if step
            .r#in
            .iter()
            .any(|v| v.id == Some(to_slot_id.to_owned()))
        {
            step.r#in.retain(|v| v.id != Some(to_slot_id.to_owned()));
            Ok(())
        } else {
            anyhow::bail!("Input {} not found in step {}!", to_slot_id, to_name);
        }
    } else {
        anyhow::bail!("Step {} not found in workflow!", to_name);
    }
}

/// Removes a connection between an output and a `CommandLineTool`.
pub fn remove_workflow_output_connection(
    workflow: &mut Workflow,
    to_output: &str,
    remove_output: bool,
) -> Result<()> {
    if remove_output
        && let Some(index) = workflow
            .outputs
            .iter()
            .position(|o| o.id == Some(to_output.to_owned()))
    {
        // Remove the output connection
        workflow.outputs.remove(index);
    } else if !remove_output
        && let Some(output) = workflow
            .outputs
            .iter_mut()
            .find(|o| o.id == Some(to_output.to_owned()))
    {
        output.output_source = None;
    }
    Ok(())
}
