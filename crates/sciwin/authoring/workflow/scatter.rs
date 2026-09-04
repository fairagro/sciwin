use crate::authoring::{AuthoringError, AuthoringResult};
use commonwl::{
    OneOrMany,
    documents::{ScatterMethod, Workflow},
    inputs::{InputSchema, InputType},
    outputs::{CommandOutputParameterType, CommandOutputType},
    requirements::{ScatterFeatureRequirement, WorkflowRequirements},
    types::CWLType,
};

/// Whether `step_id` scatters over `port` specifically.
pub fn step_scatters_over(workflow: &Workflow, step_id: &str, port: &str) -> bool {
    workflow
        .steps
        .iter()
        .find(|s| s.id.as_deref() == Some(step_id))
        .and_then(|s| s.scatter.as_ref())
        .is_some_and(|scatter| scatter.as_many().iter().any(|p| p == port))
}

pub fn step_is_scattered(workflow: &Workflow, step_id: &str) -> bool {
    workflow
        .steps
        .iter()
        .find(|s| s.id.as_deref() == Some(step_id))
        .is_some_and(|s| {
            s.scatter
                .as_ref()
                .is_some_and(|sc| !sc.as_many().is_empty())
        })
}

/// Whether `array_type` is an array whose item type is exactly
/// `scalar_type` -- the shape a source (a workflow input, or another step's
/// output already handled via [`check_slot_compatibility_scattered`]) takes
/// when the step it feeds scatters over that slot.
pub fn is_scattered_array_of(
    array_type: &OneOrMany<InputType>,
    scalar_type: &OneOrMany<InputType>,
) -> bool {
    array_type.as_many().iter().all(|t| match t {
        InputType::InputSchema(schema) => match schema.as_ref() {
            InputSchema::Array(arr) => arr.items == *scalar_type,
            _ => false,
        },
        _ => false,
    })
}

#[derive(Debug)]
pub enum ScatterProducerFit {
    Exact,
    NeedsPickValueToDropNulls,
    Incompatible,
}

/// Whether a scattered step's per-iteration `output` can feed an array-typed
/// `input`.
pub fn check_slot_compatibility_scattered_producer(
    input: &OneOrMany<InputType>,
    output: &CommandOutputParameterType,
) -> ScatterProducerFit {
    let produced = match output {
        CommandOutputParameterType::Stdout | CommandOutputParameterType::Stderr => {
            vec![CommandOutputType::CWLType(CWLType::File)]
        }
        CommandOutputParameterType::CommandOutputType(types) => types.as_many(),
    };
    let non_null: Vec<&CommandOutputType> = produced
        .iter()
        .filter(|p| !matches!(p, CommandOutputType::CWLType(CWLType::Null)))
        .collect();
    let accepted: Vec<InputType> = input.as_many();

    let fits = |items: &[&CommandOutputType]| {
        accepted.iter().any(|a| match a {
            InputType::InputSchema(schema) => match schema.as_ref() {
                InputSchema::Array(arr) => {
                    let item_types = arr.items.as_many();
                    items.iter().all(|p| {
                        item_types
                            .iter()
                            .any(|it| super::single_type_matches(p, it))
                    })
                }
                _ => false,
            },
            _ => false,
        })
    };

    let all: Vec<&CommandOutputType> = produced.iter().collect();
    if fits(&all) {
        ScatterProducerFit::Exact
    } else if non_null.len() != produced.len() && fits(&non_null) {
        ScatterProducerFit::NeedsPickValueToDropNulls
    } else {
        ScatterProducerFit::Incompatible
    }
}

/// Marks `step_id`'s `port` input as scattered, adding it to any inputs the
/// step already scatters over.
/// # Errors
/// If no step with `step_id` exists
pub fn add_step_to_scatter_mut(
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
    let mut ports = step
        .scatter
        .take()
        .map(OneOrMany::into_many)
        .unwrap_or_default();
    if !ports.iter().any(|p| p == port) {
        ports.push(port.to_string());
    }
    step.scatter = Some(match ports.len() {
        1 => OneOrMany::One(ports.into_iter().next().expect("checked len == 1")),
        _ => OneOrMany::Many(ports),
    });

    if let Some(OneOrMany::Many(_)) = step.scatter
        && step.scatter_method.is_none()
    {
        step.scatter_method = Some(ScatterMethod::Dotproduct);
    }

    //add scatter feature requirement
    workflow.append_requirement_mut(WorkflowRequirements::ScatterFeatureRequirement(
        ScatterFeatureRequirement {},
    ));
    Ok(())
}

/// Removes `step_id`'s `port` from its `scatter` list. Collapses `scatter`
/// to `None` once empty, rather than round-tripping `scatter: []`.
/// # Errors
/// If no step with `step_id` exists
pub fn remove_step_from_scatter_mut(
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
    let remaining: Vec<String> = step
        .scatter
        .take()
        .map(OneOrMany::into_many)
        .unwrap_or_default()
        .into_iter()
        .filter(|p| p != port)
        .collect();
    step.scatter = match remaining.len() {
        0 => None,
        1 => Some(OneOrMany::One(remaining.into_iter().next().unwrap())),
        _ => Some(OneOrMany::Many(remaining)),
    };

    if step.scatter.is_none() {
        step.scatter_method = None;
    }
    Ok(())
}

/// Sets or clears `step_id`'s `scatterMethod`.
/// # Errors
/// If no step with `step_id` exists
pub fn set_step_scatter_method_mut(
    workflow: &mut Workflow,
    step_id: &str,
    method: Option<ScatterMethod>,
) -> AuthoringResult<()> {
    let step = workflow
        .steps
        .iter_mut()
        .find(|s| s.id.as_deref() == Some(step_id))
        .ok_or_else(|| AuthoringError::InvalidWorkflowStep {
            id: step_id.to_string(),
        })?;
    step.scatter_method = method;
    Ok(())
}
