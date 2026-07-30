use crate::authoring::{AuthoringError, AuthoringResult};
use anyhow::{anyhow, Context};
use commonwl::{
    OneOrMany,
    documents::{CWLDocument, Workflow},
    format::format_cwl,
    inputs::{InputSchema, InputType},
    load_cwl_file,
    outputs::{CommandOutputParameterType, CommandOutputSchema, CommandOutputType},
    types::CWLType,
};
use std::{fs, path::Path};

pub fn create_workflow(filename: impl AsRef<Path>, force: bool) -> AuthoringResult<String> {
    let wf = Workflow {
        cwl_version: Some("v1.2".to_string()),
        ..Default::default()
    };
    let wf = CWLDocument::Workflow(wf);
    let filename = filename.as_ref();

    let mut yaml = serde_saphyr::to_string(&wf)?;
    yaml = format_cwl(&yaml).map_err(|e| anyhow!("Could not format yaml: {}", e))?;

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
    fs::write(filename, &yaml).with_context(|| {
        format!(
            "❌ Could not create workflow {name} at {}",
            filename.to_string_lossy(),
        )
    })?;
    Ok(yaml)
}

pub fn add_workflow_step(
    workflow: &mut Workflow,
    name: &str,
    path: impl AsRef<Path>,
    doc: &CWLDocument,
) -> AuthoringResult<()> {
    if workflow.has_step(name) {
        return Ok(()); // or do we want error?
    }
    let path = path.as_ref().to_string_lossy().into_owned();
    let path = if path.starts_with("workflows") {
        path.replace("workflows", "..")
    } else {
        format!("../../{path}")
    };
    workflow.add_workflow_step_empty_mut(name, Path::new(&path))?;
    workflow.add_workflow_step_outputs_by_doc_mut(name, doc)?;

    Ok(())
}

/// Adds a connection between an input and a `CommandLineTool`. The tool will be registered as step if it is not already and an Workflow input will be added.
pub fn add_workflow_input_connection(
    workflow: &mut Workflow,
    from_input: &str,
    to_filename: impl AsRef<Path>,
    to_name: &str,
    to_slot_id: &str,
) -> AuthoringResult<()> {
    let to_filename = to_filename.as_ref();

    let to_cwl = load_cwl_file(to_filename, true)?;
    let to_inputs = to_cwl.get_inputs();
    let to_slot = to_inputs
        .iter()
        .find(|i| i.id == Some(to_slot_id.to_owned()))
        .expect("No slot");

    //register input
    if !workflow.has_input(from_input) {
        workflow.add_workflow_input_mut(
            from_input,
            to_slot.r#type.clone(),
            to_slot.default.clone(),
        );
    }

    add_workflow_step(workflow, to_name, to_filename, &to_cwl)?;
    //add input in step
    workflow
        .add_workflow_step_input_mut(to_name, to_slot_id, OneOrMany::One(from_input.to_owned()))
        .expect("step was just added above");
    Ok(())
}

/// Adds a connection between an output and a `CommandLineTool`. The tool will be registered as step if it is not already and an Workflow output will be added.
pub fn add_workflow_output_connection(
    workflow: &mut Workflow,
    from_name: &str,
    from_slot_id: &str,
    from_filename: impl AsRef<Path>,
    to_output: &str,
) -> AuthoringResult<()> {
    let from_filename = from_filename.as_ref();

    let from_cwl = load_cwl_file(from_filename, true)?;
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
    add_workflow_step(workflow, from_name, from_filename, &from_cwl)?;

    if workflow.has_output(to_output) {
        let output = workflow
            .outputs
            .iter_mut()
            .find(|o| o.id == Some(to_output.to_owned()))
            .expect("has_output confirmed above");
        output.r#type = from_type;
        output.output_source = Some(OneOrMany::One(format!("{from_name}/{from_slot_id}")));
    } else {
        workflow.add_workflow_output_mut(
            to_output,
            from_type,
            OneOrMany::One(format!("{from_name}/{from_slot_id}")),
        );
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
) -> AuthoringResult<()> {
    //check if step already exists and create if not
    let from_filename = from_filename.as_ref();
    let to_filename = to_filename.as_ref();

    if !workflow.has_step(from_name) {
        let from_cwl = load_cwl_file(from_filename, true)?;
        let from_outputs = from_cwl.get_output_ids();
        if !from_outputs.contains(&from_slot_id.to_string()) {
            return Err(AuthoringError::InvalidWorkflowOutput {
                id: from_slot_id.to_string(),
                path: from_name.to_string(),
            });
        }

        //create step
        add_workflow_step(workflow, from_name, from_filename, &from_cwl)?;
    }

    //check if step exists
    if !workflow.has_step(to_name) {
        let to_cwl = load_cwl_file(to_filename, true)?;
        add_workflow_step(workflow, to_name, to_filename, &to_cwl)?;
    }

    workflow.add_workflow_step_input_mut(
        to_name,
        to_slot_id,
        OneOrMany::One(format!("{from_name}/{from_slot_id}")),
    )?;

    Ok(())
}

/// Removes a connection between two `CommandLineTools` by removing input from `tool_y` that is also output of `tool_x`.
pub fn remove_workflow_step_connection(
    workflow: &mut Workflow,
    to_name: &str,
    to_slot_id: &str,
) -> AuthoringResult<()> {
    workflow.remove_workflow_step_input_mut(to_name, to_slot_id)?;
    Ok(())
}

/// Removes an input from inputs and removes it from `CommandLineTool` input.
pub fn remove_workflow_input_connection(
    workflow: &mut Workflow,
    from_input: &str,
    to_name: &str,
    to_slot_id: &str,
    remove_input: bool,
) -> AuthoringResult<()> {
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
            Err(AuthoringError::InvalidWorkflowInput {
                id: to_slot_id.to_string(),
                path: format!("step {to_name}"),
            })
        }
    } else {
        Err(AuthoringError::InvalidWorkflowStep {
            id: to_name.to_string(),
        })
    }
}

/// Removes a connection between an output and a `CommandLineTool`.
pub fn remove_workflow_output_connection(
    workflow: &mut Workflow,
    to_output: &str,
    remove_output: bool,
) -> AuthoringResult<()> {
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

pub fn check_slot_compatibility(
    input: &OneOrMany<InputType>,
    output: &CommandOutputParameterType,
) -> bool {
    let produced = match output {
        CommandOutputParameterType::Stdout | CommandOutputParameterType::Stderr => {
            vec![CommandOutputType::CWLType(CWLType::File)]
        }
        CommandOutputParameterType::CommandOutputType(types) => types.as_many(),
    };
    let accepted: Vec<InputType> = input.as_many();

    produced
        .iter()
        .all(|p| accepted.iter().any(|a| single_type_matches(p, a)))
}

fn single_type_matches(output: &CommandOutputType, input: &InputType) -> bool {
    match (output, input) {
        (CommandOutputType::CWLType(o), InputType::CWLType(i)) => o == i,
        (CommandOutputType::CommandOutputSchema(o), InputType::InputSchema(i)) => {
            schema_matches(o, i)
        }
        (CommandOutputType::String(o_name), InputType::String(i_name)) => {
            local_name(o_name) == local_name(i_name)
        }
        _ => false,
    }
}

fn schema_matches(output: &CommandOutputSchema, input: &InputSchema) -> bool {
    match (output, input) {
        (CommandOutputSchema::Array(o), InputSchema::Array(i)) => {
            let produced = &o.items.as_many();
            let accepted = &i.items.as_many();
            produced
                .iter()
                .all(|p| accepted.iter().any(|a| single_type_matches(p, a)))
        }
        (CommandOutputSchema::Enum(o), InputSchema::Enum(i)) => {
            let mut o_symbols = o.symbols.clone();
            let mut i_symbols = i.symbols.clone();
            o_symbols.sort();
            i_symbols.sort();
            o_symbols == i_symbols
        }
        (CommandOutputSchema::Record(o), InputSchema::Record(i)) => {
            // Named schemas (the common case — defined once via $schemas/$graph
            // and referenced by name) are compared by name only.
            if let (Some(o_name), Some(i_name)) = (&o.name, &i.name) {
                return local_name(o_name) == local_name(i_name);
            }
            // Otherwise fall back to a structural, field-by-field comparison.
            match (&o.fields, &i.fields) {
                (Some(o_fields), Some(i_fields)) => o_fields.iter().all(|of| {
                    i_fields.iter().any(|inf| {
                        of.name == inf.name
                            && of.r#type.as_many().iter().all(|p| {
                                inf.r#type
                                    .as_many()
                                    .iter()
                                    .any(|a| single_type_matches(p, a))
                            })
                    })
                }),
                (None, None) => true,
                _ => false,
            }
        }
        _ => false,
    }
}

fn local_name(name: &str) -> &str {
    name.rsplit(['#', '/']).next().unwrap_or(name)
}
