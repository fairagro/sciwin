use commonwl::{
    OneOrMany,
    documents::{Argument, CommandLineTool},
    files::FileOrDirectory,
    inputs::{CommandInputArraySchema, CommandInputParameterType, CommandInputSchema, CommandInputType, DefaultValue},
    types::CWLType,
};
use std::collections::HashSet;

/// Applies some postprocessing to the cwl `CommandLineTool`
pub(crate) fn post_process_cwl(tool: &mut CommandLineTool) -> anyhow::Result<()> {
    detect_array_inputs(tool)?;
    post_process_variables(tool);
    post_process_ids(tool);
    Ok(())
}

/// Transforms duplicate key and type entries into an array type input
fn detect_array_inputs(tool: &mut CommandLineTool) -> anyhow::Result<()> {
    let mut seen = HashSet::new();
    let mut inputs = Vec::new();

    for input in std::mem::take(&mut tool.inputs) {
        let key = (input.id.clone(), input.r#type.clone());
        if seen.insert(key.clone()) {
            inputs.push(input);
        } else if let Some(existing) = inputs.iter_mut().find(|i| i.id == key.0) {
            // Convert to array type if not already
            if !matches!(&existing.r#type, CommandInputParameterType::CommandInputType(OneOrMany::One(CommandInputType::CommandInputSchema(schema))) if matches!(&**schema, CommandInputSchema::Array(_)) )
            {
                if let CommandInputParameterType::CommandInputType(in_ty) = input.r#type {
                    existing.r#type = CommandInputSchema::Array(CommandInputArraySchema::builder().items(in_ty).build()).into();

                    if let Some(default) = &existing.default {
                        existing.default = Some(DefaultValue::Any(serde_yaml::to_value(vec![default.clone()])?));
                    }
                }
            }

            // Append additional default value if present
            if let Some(DefaultValue::Any(serde_yaml::Value::Sequence(defaults))) = &mut existing.default
                && let Some(default) = input.default
            {
                defaults.push(serde_yaml::to_value(default.clone())?);
            }
        }
    }
    tool.inputs = inputs;
    Ok(())
}

/// Handles translation to CWL Variables like $(inputs.myInput.path) or $(runtime.outdir)
fn post_process_variables(tool: &mut CommandLineTool) {
    fn process_input(input: &CommandInputParameter) -> String {
        if input.type_ == CWLType::File || input.type_ == CWLType::Directory {
            format!("$(inputs.{}.path)", input.id)
        } else {
            format!("$(inputs.{})", input.id)
        }
    }

    let mut processed_once = false;
    let inputs = tool.inputs.clone();
    for input in &inputs {
        if let Some(default) = &input.default {
            for output in &mut tool.outputs {
                if let Some(binding) = &mut output.output_binding
                    && binding.glob == Some(OneOrMany::One(default_string(default)))
                {
                    binding.glob = Some(OneOrMany::One(process_input(input)));
                    processed_once = true;
                }
            }
            if let Some(stdout) = &tool.stdout
                && *stdout == default_string(default)
            {
                tool.stdout = Some(process_input(input));
                processed_once = true;
            }
            if let Some(stderr) = &tool.stderr
                && *stderr == default_string(default)
            {
                tool.stderr = Some(process_input(input));
                processed_once = true;
            }

            if let Some(arguments) = &mut tool.arguments {
                for argument in arguments.iter_mut() {
                    match argument {
                        Argument::String(s) => {
                            if *s == default_string(default) {
                                *s = process_input(input);
                                processed_once = true;
                            }
                        }
                        Argument::Binding(binding) => {
                            if let Some(from) = &mut binding.value_from
                                && *from == default_string(default)
                            {
                                *from = process_input(input);
                                processed_once = true;
                            }
                        }
                    }
                }
            }
        }
    }

    for output in &mut tool.outputs {
        if let Some(binding) = &mut output.output_binding
            && matches!(binding.glob, Some(SingularPlural::Singular(ref s)) if s == ".")
        {
            output.id = Some("output_directory".to_string());
            binding.glob = Some(SingularPlural::Singular("$(runtime.outdir)".to_string()));
        }
    }

    if processed_once && let Some(reqs) = &mut tool.requirements {
        reqs.push(ToolRequirements::InlineJavascriptRequirement(InlineJavascriptRequirement::default()));
    }
}

/// Post-processes output IDs to ensure they do not conflict with input IDs
fn post_process_ids(tool: &mut CommandLineTool) {
    let input_ids = tool.inputs.iter().map(|i| i.id.clone()).collect::<HashSet<_>>();
    for output in &mut tool.outputs {
        if input_ids.contains(&output.id) {
            output.id = Some(format!("o_{}", output.id.as_ref().unwrap()));
        }
    }
}

fn default_string(default: &DefaultValue) -> String {
    match default {
        DefaultValue::FileOrDirectory(FileOrDirectory::File(f)) => f.path.clone().unwrap(),
        DefaultValue::FileOrDirectory(FileOrDirectory::Directory(d)) => d.path.clone().unwrap(),
        DefaultValue::Any(value) => value.as_str().unwrap_or_default().to_string(), //??
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use commonwl::inputs::CommandInputParameter;
    use serde_yaml::Value;

    #[test]
    pub fn test_post_process_inputs() {
        let mut tool = CommandLineTool::builder()
            .inputs(vec![
                CommandInputParameter::builder()
                    .id("arr")
                    .r#type(CWLType::String)
                    .default(DefaultValue::Any(Value::String("first".to_string())))
                    .build(),
                CommandInputParameter::builder()
                    .id("arr")
                    .r#type(CWLType::String)
                    .default(DefaultValue::Any(Value::String("second".to_string())))
                    .build(),
                CommandInputParameter::builder()
                    .id("arr")
                    .r#type(CWLType::String)
                    .default(DefaultValue::Any(Value::String("third".to_string())))
                    .build(),
                CommandInputParameter::builder()
                    .id("int")
                    .r#type(CWLType::Int)
                    .default(DefaultValue::Any(Value::String("fourth".to_string())))
                    .build(),
            ])
            .build();

        assert_eq!(tool.inputs.len(), 4);
        detect_array_inputs(&mut tool);
        assert_eq!(tool.inputs.len(), 2);

        let of_interest = tool.inputs.first().unwrap();
        assert_eq!(
            of_interest.r#type,
            CommandInputParameterType::CommandInputType(OneOrMany::One(CommandInputType::CommandInputSchema(Box::new(CommandInputSchema::Array(
                CommandInputArraySchema::builder()
                    .items(OneOrMany::One(CommandInputType::CWLType(CWLType::String)))
                    .build()
            )))))
        );
        assert_eq!(
            of_interest.default,
            Some(DefaultValue::Any(
                serde_yaml::to_value(vec![
                    DefaultValue::Any(Value::String("first".to_string())),
                    DefaultValue::Any(Value::String("second".to_string())),
                    DefaultValue::Any(Value::String("third".to_string()))
                ])
                .unwrap()
            ))
        );

        let other = &tool.inputs[1];
        assert_eq!(other.r#type, CWLType::Int.into());
    }
}
