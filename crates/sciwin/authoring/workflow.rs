use crate::authoring::{AuthoringError, AuthoringResult, paths};
use anyhow::Context;
use commonwl::{
    OneOrMany,
    documents::{CWLDocument, Workflow},
    format::format_cwl,
    inputs::{InputSchema, InputType},
    load_cwl_file,
    outputs::{CommandOutputParameterType, CommandOutputSchema, CommandOutputType},
    types::CWLType,
};
use std::{
    fs,
    path::{Path, PathBuf},
};

/// Creates a blank workflow document named `name`.
///
/// Follows the same path scheme as [`crate::authoring::tool::create_tool`]: `output_dir` is
/// the project folder to place it in; when absent, it falls back to a per-workflow folder
/// under [`paths::WORKFLOWS_FOLDER`].
pub fn create_workflow(
    name: &str,
    output_dir: Option<PathBuf>,
    force: bool,
) -> AuthoringResult<(PathBuf, String)> {
    let wf = Workflow {
        cwl_version: Some("v1.2".to_string()),
        ..Default::default()
    };
    let wf = CWLDocument::Workflow(wf);

    let mut yaml = serde_saphyr::to_string(&wf)?;
    yaml = format_cwl(&yaml).context("Could not format yaml")?;

    let base_dir = output_dir.unwrap_or_else(|| Path::new(paths::WORKFLOWS_FOLDER).join(name));
    let path = paths::get_qualified_filename_by_name(name, base_dir);

    //removes file first if exists and force is given
    if force && path.exists() {
        fs::remove_file(&path)?;
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create directories for {}", parent.display()))?;
    }
    fs::write(&path, &yaml).with_context(|| {
        format!(
            "❌ Could not create workflow {name} at {}",
            path.to_string_lossy(),
        )
    })?;
    Ok((path, yaml))
}

/// One end of a workflow connection: the CWL document at `filename`, registered as a step
/// named `name`, wired through its `slot_id` input/output.
#[derive(Clone, Copy)]
pub struct WorkflowSlot<'a> {
    pub filename: &'a Path,
    pub name: &'a str,
    pub slot_id: &'a str,
}

impl<'a> WorkflowSlot<'a> {
    pub fn new(filename: &'a Path, name: &'a str, slot_id: &'a str) -> Self {
        Self {
            filename,
            name,
            slot_id,
        }
    }
}

/// Registers `path` (the tool's own location) as a step, relative to `workflow_path` -- the
/// location the workflow itself will be saved at. Neither has to live under
/// [`paths::WORKFLOWS_FOLDER`] or at any fixed depth: the tool and the workflow can each be
/// anywhere in the project, so the step's `run` path is computed by diffing the two real
/// locations rather than assumed from a folder convention.
pub fn add_workflow_step(
    workflow: &mut Workflow,
    workflow_path: impl AsRef<Path>,
    name: &str,
    path: impl AsRef<Path>,
    doc: &CWLDocument,
) -> AuthoringResult<()> {
    if workflow.has_step(name) {
        return Ok(()); // or do we want error?
    }
    let path = paths::resolve_path(path, workflow_path);
    workflow.add_workflow_step_empty_mut(name, Path::new(&path))?;
    workflow.add_workflow_step_outputs_by_doc_mut(name, doc)?;

    Ok(())
}

/// Adds a connection between an input and a `CommandLineTool`. The tool will be registered as step if it is not already and an Workflow input will be added.
/// # Errors
/// If `from_input` already exists with a type incompatible with `to`'s slot -- a fresh input
/// takes on the slot's type by construction, so there's nothing to check there; an existing
/// one might already be feeding a differently-typed step.
pub fn add_workflow_input_connection(
    workflow: &mut Workflow,
    workflow_path: impl AsRef<Path>,
    from_input: &str,
    to: WorkflowSlot,
) -> AuthoringResult<()> {
    let to_cwl = load_cwl_file(to.filename, true)?;
    let to_inputs = to_cwl.get_inputs();
    let to_slot = to_inputs
        .iter()
        .find(|i| i.id.as_deref() == Some(to.slot_id))
        .expect("No slot");

    //register input
    if let Some(existing) = workflow.inputs.iter().find(|i| i.id.as_deref() == Some(from_input)) {
        if existing.r#type != to_slot.r#type {
            return Err(AuthoringError::IncompatibleType {
                message: format!(
                    "input {from_input} already has type {:?}, but {}/{} expects {:?}",
                    existing.r#type, to.name, to.slot_id, to_slot.r#type
                ),
            });
        }
    } else {
        workflow.add_workflow_input_mut(
            from_input,
            to_slot.r#type.clone(),
            to_slot.default.clone(),
        );
    }

    add_workflow_step(workflow, workflow_path, to.name, to.filename, &to_cwl)?;
    //add input in step
    workflow
        .add_workflow_step_input_mut(to.name, to.slot_id, OneOrMany::One(from_input.to_owned()))
        .expect("step was just added above");
    Ok(())
}

/// Adds a connection between an output and a `CommandLineTool`. The tool will be registered as step if it is not already and an Workflow output will be added.
/// # Errors
/// If `to_output` already exists with a type incompatible with `from`'s slot -- a fresh output
/// takes on the slot's type by construction, so there's nothing to check there; an existing
/// one might already be fed by a differently-typed source.
pub fn add_workflow_output_connection(
    workflow: &mut Workflow,
    workflow_path: impl AsRef<Path>,
    from: WorkflowSlot,
    to_output: &str,
) -> AuthoringResult<()> {
    let from_cwl = load_cwl_file(from.filename, true)?;
    let from_type = match &from_cwl {
        CWLDocument::CommandLineTool(clt) => clt
            .outputs
            .iter()
            .find(|i| i.id.as_deref() == Some(from.slot_id))
            .map(|i| i.r#type.clone()),
        CWLDocument::ExpressionTool(et) => et
            .outputs
            .iter()
            .find(|i| i.id.as_deref() == Some(from.slot_id))
            .map(|i| i.r#type.clone().into()),
        CWLDocument::Operation(op) => op
            .outputs
            .iter()
            .find(|i| i.id.as_deref() == Some(from.slot_id))
            .map(|i| i.r#type.clone().into()),
        CWLDocument::Workflow(wf) => wf
            .outputs
            .iter()
            .find(|i| i.id.as_deref() == Some(from.slot_id))
            .map(|i| i.r#type.clone()),
    }
    .expect("No slot");

    // Checked before any mutation, refusing here must not leave a step
    // registered with nothing wired to it, the way checking after
    // `add_workflow_step` used to.
    if let Some(output) = workflow.outputs.iter().find(|o| o.id.as_deref() == Some(to_output))
        && output.r#type != from_type
    {
        return Err(AuthoringError::IncompatibleType {
            message: format!(
                "output {to_output} already has type {:?}, but {}/{} produces {:?}",
                output.r#type, from.name, from.slot_id, from_type
            ),
        });
    }

    add_workflow_step(workflow, workflow_path, from.name, from.filename, &from_cwl)?;

    let source = format!("{}/{}", from.name, from.slot_id);
    if workflow.has_output(to_output) {
        // Merge into the existing sources instead of overwriting them,
        // `output_source = Some(OneOrMany::One(source))` used to drop every
        // other source already feeding this output.
        let output = workflow
            .outputs
            .iter_mut()
            .find(|o| o.id.as_deref() == Some(to_output))
            .expect("found above");
        let mut sources = output.output_source.take().map(OneOrMany::into_many).unwrap_or_default();
        if !sources.contains(&source) {
            sources.push(source);
        }
        output.output_source = Some(OneOrMany::Many(sources));
    } else {
        workflow.add_workflow_output_mut(to_output, from_type, OneOrMany::One(source));
    }

    Ok(())
}

/// Adds a connection between two `CommandLineTools`. The tools will be registered as step if registered not already.
pub fn add_workflow_step_connection(
    workflow: &mut Workflow,
    workflow_path: impl AsRef<Path>,
    from: WorkflowSlot,
    to: WorkflowSlot,
) -> AuthoringResult<()> {
    let workflow_path = workflow_path.as_ref();

    //check if step already exists and create if not
    if !workflow.has_step(from.name) {
        let from_cwl = load_cwl_file(from.filename, true)?;
        let from_outputs = from_cwl.get_output_ids();
        if !from_outputs.iter().any(|s| s.as_str() == from.slot_id) {
            return Err(AuthoringError::InvalidWorkflowOutput {
                id: from.slot_id.to_string(),
                path: from.name.to_string(),
            });
        }

        //create step
        add_workflow_step(workflow, workflow_path, from.name, from.filename, &from_cwl)?;
    }

    //check if step exists
    if !workflow.has_step(to.name) {
        let to_cwl = load_cwl_file(to.filename, true)?;
        add_workflow_step(workflow, workflow_path, to.name, to.filename, &to_cwl)?;
    }

    workflow.add_workflow_step_input_mut(
        to.name,
        to.slot_id,
        OneOrMany::One(format!("{}/{}", from.name, from.slot_id)),
    )?;

    Ok(())
}

/// Removes one connection between two `CommandLineTools`
pub fn remove_workflow_step_connection(
    workflow: &mut Workflow,
    from_name: &str,
    from_slot_id: &str,
    to_name: &str,
    to_slot_id: &str,
) -> AuthoringResult<()> {
    let source = format!("{from_name}/{from_slot_id}");
    workflow.remove_workflow_step_input_source_mut(to_name, to_slot_id, &source)?;
    Ok(())
}

/// Removes an input from inputs and removes it from `CommandLineTool` input, leaving any other
/// sources on `to_slot_id` (a multi-source input) intact.
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
            .position(|s| s.id.as_deref() == Some(from_input))
    {
        workflow.inputs.remove(index);
    }
    let Some(step) = workflow
        .steps
        .iter()
        .find(|s| s.id.as_deref() == Some(to_name))
    else {
        return Err(AuthoringError::InvalidWorkflowStep {
            id: to_name.to_string(),
        });
    };
    if !step
        .r#in
        .iter()
        .any(|v| v.id.as_deref() == Some(to_slot_id))
    {
        return Err(AuthoringError::InvalidWorkflowInput {
            id: to_slot_id.to_string(),
            path: format!("step {to_name}"),
        });
    }
    workflow.remove_workflow_step_input_source_mut(to_name, to_slot_id, from_input)?;
    Ok(())
}

/// Removes a connection between an output and a `CommandLineTool`.
pub fn remove_workflow_output_connection(
    workflow: &mut Workflow,
    to_output: &str,
    remove_output: bool,
) -> AuthoringResult<()> {
    if remove_output {
        if let Some(index) = workflow
            .outputs
            .iter()
            .position(|o| o.id.as_deref() == Some(to_output))
        {
            workflow.outputs.remove(index);
        }
    } else if let Some(output) = workflow
        .outputs
        .iter_mut()
        .find(|o| o.id.as_deref() == Some(to_output))
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

#[cfg(test)]
mod tests {
    use super::*;
    use commonwl::{
        documents::{CWLDocument, CommandLineTool, StringOrDocument, Workflow},
        inputs::CommandInputParameter,
        outputs::CommandOutputParameter,
        types::CWLType,
    };
    use std::{fs, path::Path};
    use tempfile::tempdir;

    fn os_path(path: &str) -> String {
        if cfg!(target_os = "windows") {
            Path::new(path).to_string_lossy().replace('/', "\\")
        } else {
            path.to_string()
        }
    }

    fn write_tool(path: &Path, input: &str, output: &str) {
        write_tool_typed(path, input, output, CWLType::String);
    }

    fn write_tool_typed(path: &Path, input: &str, output: &str, ty: CWLType) {
        let tool = CommandLineTool::builder()
            .cwl_version("v1.2")
            .inputs(vec![
                CommandInputParameter::builder()
                    .id(input)
                    .r#type(ty)
                    .build(),
            ])
            .outputs(vec![
                CommandOutputParameter::builder()
                    .id(output)
                    .r#type(ty)
                    .build(),
            ])
            .build();

        let yaml = serde_saphyr::to_string(&CWLDocument::CommandLineTool(tool)).unwrap();
        fs::write(path, yaml).unwrap();
    }

    #[test]
    fn create_workflow_creates_file_in_given_project_folder() {
        let dir = tempdir().unwrap();

        let (path, _) = create_workflow("workflow", Some(dir.path().to_path_buf()), false).unwrap();

        // a given output_dir is used as-is, no extra per-workflow subfolder
        assert_eq!(path, dir.path().join("workflow.cwl"));
        assert!(path.exists());

        let doc = load_cwl_file(&path, true).unwrap();
        assert!(matches!(doc, CWLDocument::Workflow(_)));
        assert_eq!(doc.cwl_version(), Some(&"v1.2".to_string()));
    }

    #[fstest::fstest]
    fn create_workflow_falls_back_to_workflows_folder() {
        // no output_dir given -> workflows/<name>/<name>.cwl, same convention as create_tool
        let (path, _) = create_workflow("myworkflow", None, false).unwrap();

        assert_eq!(
            path,
            Path::new("workflows")
                .join("myworkflow")
                .join("myworkflow.cwl")
        );
        assert!(path.exists());
    }

    #[test]
    fn add_step_registers_step_and_outputs() {
        let dir = tempdir().unwrap();
        // workflow and tool live in sibling subfolders, not under a shared "workflows/" root --
        // the step path must be computed relative to the workflow, not assumed from a convention
        let tool_path = dir.path().join("tools").join("tool.cwl");
        let workflow_path = dir.path().join("pipelines").join("wf.cwl");

        fs::create_dir_all(tool_path.parent().unwrap()).unwrap();
        write_tool(&tool_path, "in", "out");

        let mut wf = Workflow::default();
        let doc = load_cwl_file(&tool_path, true).unwrap();

        add_workflow_step(&mut wf, &workflow_path, "tool", &tool_path, &doc).unwrap();

        assert!(wf.has_step("tool"));

        let step = wf.get_step("tool").unwrap();
        assert_eq!(step.out.len(), 1);
        assert_eq!(step.out[0].id(), "out");
        assert_eq!(
            step.run,
            StringOrDocument::String(os_path("../tools/tool.cwl"))
        );
    }

    #[test]
    fn connect_workflow_input() {
        let dir = tempdir().unwrap();
        let tool_path = dir.path().join("tool.cwl");
        let workflow_path = dir.path().join("workflow.cwl");

        write_tool(&tool_path, "message", "out");

        let mut wf = Workflow::default();

        add_workflow_input_connection(
            &mut wf,
            &workflow_path,
            "workflow_input",
            WorkflowSlot::new(&tool_path, "tool", "message"),
        )
        .unwrap();

        assert!(wf.has_input("workflow_input"));
        assert!(wf.has_step("tool"));

        let step = wf.get_step("tool").unwrap();
        assert_eq!(step.r#in.len(), 1);
        assert_eq!(
            step.r#in[0].source.as_ref().unwrap().as_many(),
            vec!["workflow_input".to_string()]
        );
    }

    #[test]
    fn connect_workflow_input_reuses_existing_input_with_matching_type() {
        let dir = tempdir().unwrap();
        let tool_a = dir.path().join("a.cwl");
        let tool_b = dir.path().join("b.cwl");
        let workflow_path = dir.path().join("workflow.cwl");

        write_tool(&tool_a, "message", "out");
        write_tool(&tool_b, "message", "out");

        let mut wf = Workflow::default();
        add_workflow_input_connection(
            &mut wf,
            &workflow_path,
            "workflow_input",
            WorkflowSlot::new(&tool_a, "a", "message"),
        )
        .unwrap();
        add_workflow_input_connection(
            &mut wf,
            &workflow_path,
            "workflow_input",
            WorkflowSlot::new(&tool_b, "b", "message"),
        )
        .unwrap();

        // one input, feeding both steps -- not duplicated, not overwritten
        assert_eq!(wf.inputs.iter().filter(|i| i.id.as_deref() == Some("workflow_input")).count(), 1);
        assert!(wf.has_step("a"));
        assert!(wf.has_step("b"));
    }

    #[test]
    fn connect_workflow_input_refuses_type_mismatch_with_existing_input() {
        let dir = tempdir().unwrap();
        let string_tool = dir.path().join("a.cwl");
        let file_tool = dir.path().join("b.cwl");
        let workflow_path = dir.path().join("workflow.cwl");

        write_tool(&string_tool, "message", "out");
        write_tool_typed(&file_tool, "message", "out", CWLType::File);

        let mut wf = Workflow::default();
        add_workflow_input_connection(
            &mut wf,
            &workflow_path,
            "workflow_input",
            WorkflowSlot::new(&string_tool, "a", "message"),
        )
        .unwrap();

        let result = add_workflow_input_connection(
            &mut wf,
            &workflow_path,
            "workflow_input",
            WorkflowSlot::new(&file_tool, "b", "message"),
        );

        assert!(matches!(result, Err(AuthoringError::IncompatibleType { .. })));
        // refused before wiring anything to the second step
        assert!(!wf.has_step("b"));
    }

    #[test]
    fn connect_workflow_output() {
        let dir = tempdir().unwrap();
        let tool_path = dir.path().join("tool.cwl");
        let workflow_path = dir.path().join("workflow.cwl");

        write_tool(&tool_path, "in", "result");

        let mut wf = Workflow::default();

        add_workflow_output_connection(
            &mut wf,
            &workflow_path,
            WorkflowSlot::new(&tool_path, "tool", "result"),
            "final_result",
        )
        .unwrap();

        assert!(wf.has_output("final_result"));

        let output = wf
            .outputs
            .iter()
            .find(|o| o.id.as_deref() == Some("final_result"))
            .unwrap();

        assert_eq!(
            output.output_source.as_ref().unwrap().as_many(),
            vec!["tool/result".to_string()]
        );
    }

    #[test]
    fn connect_workflow_output_merges_sources_with_matching_type() {
        let dir = tempdir().unwrap();
        let tool_a = dir.path().join("a.cwl");
        let tool_b = dir.path().join("b.cwl");
        let workflow_path = dir.path().join("workflow.cwl");

        write_tool(&tool_a, "in", "result");
        write_tool(&tool_b, "in", "result");

        let mut wf = Workflow::default();
        add_workflow_output_connection(
            &mut wf,
            &workflow_path,
            WorkflowSlot::new(&tool_a, "a", "result"),
            "final_result",
        )
        .unwrap();
        add_workflow_output_connection(
            &mut wf,
            &workflow_path,
            WorkflowSlot::new(&tool_b, "b", "result"),
            "final_result",
        )
        .unwrap();

        let output = wf
            .outputs
            .iter()
            .find(|o| o.id.as_deref() == Some("final_result"))
            .unwrap();
        assert_eq!(
            output.output_source.as_ref().unwrap().as_many(),
            vec!["a/result".to_string(), "b/result".to_string()],
            "the second connection must not overwrite the first"
        );
    }

    #[test]
    fn connect_workflow_output_refuses_type_mismatch_with_existing_output() {
        let dir = tempdir().unwrap();
        let string_tool = dir.path().join("a.cwl");
        let file_tool = dir.path().join("b.cwl");
        let workflow_path = dir.path().join("workflow.cwl");

        write_tool(&string_tool, "in", "result");
        write_tool_typed(&file_tool, "in", "result", CWLType::File);

        let mut wf = Workflow::default();
        add_workflow_output_connection(
            &mut wf,
            &workflow_path,
            WorkflowSlot::new(&string_tool, "a", "result"),
            "final_result",
        )
        .unwrap();

        let result = add_workflow_output_connection(
            &mut wf,
            &workflow_path,
            WorkflowSlot::new(&file_tool, "b", "result"),
            "final_result",
        );

        assert!(matches!(result, Err(AuthoringError::IncompatibleType { .. })));
        // refused before wiring anything, or registering the second step
        assert!(!wf.has_step("b"));
    }

    #[test]
    fn connect_two_steps() {
        let dir = tempdir().unwrap();

        let workflow_path = dir.path().join("workflow.cwl");
        let producer = dir.path().join("producer.cwl");
        let consumer = dir.path().join("consumer.cwl");

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

        assert!(wf.has_step("producer"));
        assert!(wf.has_step("consumer"));

        let step = wf.get_step("consumer").unwrap();

        assert_eq!(
            step.r#in[0].source.as_ref().unwrap().as_many(),
            vec!["producer/value".to_string()]
        );
    }

    #[test]
    fn remove_connections() {
        let dir = tempdir().unwrap();
        let tool_path = dir.path().join("tool.cwl");
        let workflow_path = dir.path().join("workflow.cwl");

        write_tool(&tool_path, "in", "out");

        let mut wf = Workflow::default();

        add_workflow_input_connection(
            &mut wf,
            &workflow_path,
            "wf_in",
            WorkflowSlot::new(&tool_path, "tool", "in"),
        )
        .unwrap();
        add_workflow_output_connection(
            &mut wf,
            &workflow_path,
            WorkflowSlot::new(&tool_path, "tool", "out"),
            "wf_out",
        )
        .unwrap();

        remove_workflow_input_connection(&mut wf, "wf_in", "tool", "in", true).unwrap();
        remove_workflow_output_connection(&mut wf, "wf_out", true).unwrap();

        assert!(!wf.has_input("wf_in"));
        assert!(!wf.has_output("wf_out"));

        let step = wf.get_step("tool").unwrap();
        assert!(step.r#in.is_empty());
    }
}
