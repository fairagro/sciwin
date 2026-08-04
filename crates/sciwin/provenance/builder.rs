//! Turns a [`CrateInputs`] into an [`RoCrate`]
//! `CrateInputs::date_published` stands in for `Utc::now()`

use std::collections::{BTreeMap, HashMap, HashSet};

use rocrate::{RoCrate, build::Entity, validate::Validation};

use crate::provenance::{
    ProvenanceResult,
    graph::{PortKind, PortNode, StepNode, WorkflowGraph},
    inputs::{CrateInputs, RunRecord},
};

const CWL_ID: &str = "https://w3id.org/workflowhub/workflow-ro-crate#cwl";
const DOCKER_IMAGE_IRI: &str = "https://w3id.org/ro/terms/workflow-run#DockerImage";
const ENGINE_ID: &str = "#engine";

/// Builds the crate. Errors only on malformed input (no `Workflow` in the packed graph, or a
/// step whose `run` does not resolve) -- a crate that breaks a profile's `Must` rules is still
/// returned, not rejected; inspect it with [`RoCrate::validate`] and decide what to do with the
/// violations at the call site (the CLI, not this library, is where those get surfaced).
///
/// # Errors
/// See [`crate::provenance::ProvenanceError`].
pub fn build_crate(inputs: &CrateInputs) -> ProvenanceResult<RoCrate> {
    let graph = WorkflowGraph::from_packed(&inputs.workflow)?;
    let wf = inputs.workflow_file.as_str();
    let run = &inputs.run;

    let mut rocrate = RoCrate::builder()
        .context(inputs.context.clone())
        .date_published(inputs.date_published.to_rfc3339())
        .name(inputs.metadata.name.clone())
        .maybe_description(inputs.metadata.description.clone())
        .maybe_license(inputs.metadata.license.clone());
    for profile in &inputs.profiles {
        rocrate = rocrate.conforms_to(profile.clone());
    }

    let cwl_version = inputs
        .workflow
        .cwl_version
        .clone()
        .unwrap_or_else(|| "v1.2".to_string());
    rocrate = rocrate.entity(
        Entity::new(CWL_ID, "ComputerLanguage")
            .set("name", "Common Workflow Language")
            .set("alternateName", "CWL")
            .reference("identifier", "https://w3id.org/cwl/v1.2/")
            .reference("url", "https://www.commonwl.org/")
            .set("version", cwl_version),
    );

    // Distinct tools the workflow's steps run, in first-seen order.
    let mut tool_ids: Vec<&str> = Vec::new();
    for step in &graph.steps {
        if !tool_ids.contains(&step.run.as_str()) {
            tool_ids.push(&step.run);
        }
    }

    // FormalParameters, deduplicated by id -- a badly-authored tool can reuse the same port id
    // for both its input and output
    let mut formal_parameters: Vec<&PortNode> = Vec::new();
    let mut seen_parameters: HashSet<&str> = HashSet::new();
    for port in all_ports(&graph) {
        if seen_parameters.insert(&port.id) {
            formal_parameters.push(port);
        }
    }
    for port in &formal_parameters {
        let mut entity = Entity::new(prefixed(wf, &port.id), "FormalParameter")
            .set("name", last_segment(&port.id));
        if let Some(ty) = port.additional_type {
            entity = entity.set("additionalType", ty.to_string());
        }
        rocrate = rocrate.entity(entity);
    }

    // File entities, one per distinct crate-relative file name
    let mut file_parameters: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for port in all_ports(&graph) {
        if let Some(file_name) = &port.file_name {
            file_parameters
                .entry(file_name.as_str())
                .or_default()
                .push(prefixed(wf, &port.id));
        }
    }
    for (file_name, parameter_ids) in &file_parameters {
        let payload = inputs.payload.iter().find(|p| p.name == *file_name);
        let mut entity = Entity::new(*file_name, "File")
            .set("alternateName", *file_name)
            .references("exampleOfWork", parameter_ids.clone());
        if let Some(size) = payload.and_then(|p| p.size) {
            entity = entity.set("contentSize", size);
        }
        if let Some(checksum) = payload.and_then(|p| p.checksum.clone()) {
            entity = entity.set("sha1", checksum);
        }
        rocrate = rocrate.part(entity);
    }

    // Connections, derived not basename-matched, given stable ids by position.
    let connections: Vec<(String, String, String)> = graph
        .connections
        .iter()
        .enumerate()
        .map(|(i, (source, target))| {
            (
                format!("#connection/{i}"),
                prefixed(wf, source),
                prefixed(wf, target),
            )
        })
        .collect();

    // Container images, one per distinct pulled image actually used across all steps.
    let mut image_ids: HashMap<String, String> = HashMap::new();
    for step in &graph.steps {
        let Some(image) = actual_container_image(run, step) else {
            continue;
        };
        if image_ids.contains_key(&image) {
            continue;
        }
        let (name, tag) = image.split_once(':').unwrap_or((image.as_str(), "latest"));
        let id = format!("#image/{name}/{tag}");
        rocrate = rocrate.entity(
            Entity::new(id.clone(), "ContainerImage")
                .reference("additionalType", DOCKER_IMAGE_IRI)
                .set("name", name.to_string())
                .set("tag", tag.to_string())
                .set("registry", "docker.io"),
        );
        image_ids.insert(image, id);
    }

    let engine_id = ENGINE_ID.to_string();
    let mut engine_entity =
        Entity::new(ENGINE_ID, "SoftwareApplication").set("name", run.engine.name.clone());
    if let Some(version) = &run.engine.version {
        engine_entity = engine_entity.set("softwareVersion", version.clone());
    }
    rocrate = rocrate.entity(engine_entity);

    for run_id in &tool_ids {
        let prefix = format!("{run_id}/");
        let tool_inputs: Vec<String> = graph
            .tool_ports
            .iter()
            .filter(|p| p.kind == PortKind::Input && p.id.starts_with(&prefix))
            .map(|p| prefixed(wf, &p.id))
            .collect();
        let tool_outputs: Vec<String> = graph
            .tool_ports
            .iter()
            .filter(|p| p.kind == PortKind::Output && p.id.starts_with(&prefix))
            .map(|p| prefixed(wf, &p.id))
            .collect();
        rocrate = rocrate.entity(
            Entity::new(prefixed(wf, run_id), "SoftwareApplication")
                .set("name", run_id.trim_start_matches('#'))
                .references("input", tool_inputs)
                .references("output", tool_outputs),
        );
    }

    let workflow_id = prefixed(wf, &graph.workflow_id);
    let workflow_inputs: Vec<String> = graph.inputs.iter().map(|p| prefixed(wf, &p.id)).collect();
    let workflow_outputs: Vec<String> = graph.outputs.iter().map(|p| prefixed(wf, &p.id)).collect();
    let step_refs: Vec<String> = graph.steps.iter().map(|s| prefixed(wf, &s.id)).collect();
    let has_part: Vec<String> = tool_ids.iter().map(|id| prefixed(wf, id)).collect();
    let workflow_connection_ids: Vec<String> = connections
        .iter()
        .filter(|(_, _, target)| workflow_outputs.contains(target))
        .map(|(id, _, _)| id.clone())
        .collect();

    rocrate = rocrate.main_workflow(
        Entity::new(
            workflow_id.clone(),
            &[
                "File",
                "SoftwareSourceCode",
                "ComputationalWorkflow",
                "HowTo",
            ],
        )
        .set("name", wf)
        .reference("programmingLanguage", CWL_ID)
        .references("hasPart", has_part)
        .references("input", workflow_inputs)
        .references("output", workflow_outputs.clone())
        .references("step", step_refs)
        .references("connection", workflow_connection_ids),
    );

    // Per-step HowToStep, CreateAction and ControlAction.
    let mut organize_objects: Vec<String> = Vec::new();
    for step in &graph.steps {
        let step_run = run.steps.get(&step.id);
        let step_prefix = format!("{}/", step.run);
        let target_prefix = format!("{wf}{step_prefix}");

        let step_connection_ids: Vec<String> = connections
            .iter()
            .filter(|(_, _, target)| target.starts_with(&target_prefix))
            .map(|(id, _, _)| id.clone())
            .collect();
        rocrate = rocrate.entity(
            Entity::new(prefixed(wf, &step.id), "HowToStep")
                .set("position", step.position as u64)
                .references("connection", step_connection_ids)
                .reference("workExample", prefixed(wf, &step.run)),
        );

        let step_inputs: Vec<String> = graph
            .tool_ports
            .iter()
            .filter(|p| p.kind == PortKind::Input && p.id.starts_with(&step_prefix))
            .filter_map(|p| p.file_name.clone())
            .collect();
        let step_outputs: Vec<String> = graph
            .tool_ports
            .iter()
            .filter(|p| p.kind == PortKind::Output && p.id.starts_with(&step_prefix))
            .filter_map(|p| p.file_name.clone())
            .collect();

        let create_id = format!("#run/{}", step.id.trim_start_matches('#'));
        let mut create_action = Entity::new(create_id.clone(), "CreateAction")
            .set("name", format!("Run of {}", prefixed(wf, &step.id)))
            .reference("instrument", prefixed(wf, &step.run))
            .references("object", step_inputs)
            .references("result", step_outputs);
        if let Some(started) = step_run.and_then(|s| s.started_at) {
            create_action = create_action.set("startTime", started.to_rfc3339());
        }
        if let Some(ended) = step_run.and_then(|s| s.ended_at) {
            create_action = create_action.set("endTime", ended.to_rfc3339());
        }
        if let Some(image_id) =
            actual_container_image(run, step).and_then(|img| image_ids.get(&img))
        {
            create_action = create_action.reference("containerImage", image_id.clone());
        }
        rocrate = rocrate.mention(create_action);

        let control_id = format!("#orchestrate/{}", step.id.trim_start_matches('#'));
        rocrate = rocrate.mention(
            Entity::new(control_id.clone(), "ControlAction")
                .set(
                    "name",
                    format!("orchestrate {}", step.id.trim_start_matches('#')),
                )
                .reference("instrument", prefixed(wf, &step.id))
                .reference("object", create_id),
        );
        organize_objects.push(control_id);
    }

    // The workflow-level run: its instrument is the main workflow entity itself, which is what
    // satisfies `wfrun::workflow-run`.
    let workflow_input_files: Vec<String> = graph
        .inputs
        .iter()
        .filter_map(|p| p.file_name.clone())
        .collect();
    let workflow_output_files: Vec<String> = graph
        .outputs
        .iter()
        .filter_map(|p| p.file_name.clone())
        .collect();
    let workflow_create_id = format!("#run/{}", graph.workflow_id.trim_start_matches('#'));
    let mut workflow_create_action = Entity::new(workflow_create_id.clone(), "CreateAction")
        .set("name", format!("Run of {wf}"))
        .reference("instrument", workflow_id)
        .references("object", workflow_input_files)
        .references("result", workflow_output_files);
    if let Some(started) = run.started_at {
        workflow_create_action = workflow_create_action.set("startTime", started.to_rfc3339());
    }
    if let Some(ended) = run.ended_at {
        workflow_create_action = workflow_create_action.set("endTime", ended.to_rfc3339());
    }
    rocrate = rocrate.mention(workflow_create_action);

    // The engine's orchestration of the whole run. Always emitted, unlike the old generator,
    // which silently dropped it (and every File entity) when it could not regex an engine
    // version out of the logs.
    let organize_name = match &run.engine.version {
        Some(version) => format!("Run of {} {version}", run.engine.name),
        None => format!("Run of {}", run.engine.name),
    };
    let mut organize_action = Entity::new("#organize", "OrganizeAction")
        .set("name", organize_name)
        .reference("instrument", engine_id)
        .references("object", organize_objects)
        .references("result", vec![workflow_create_id]);
    if let Some(started) = run.started_at {
        organize_action = organize_action.set("startTime", started.to_rfc3339());
    }
    if let Some(ended) = run.ended_at {
        organize_action = organize_action.set("endTime", ended.to_rfc3339());
    }
    rocrate = rocrate.mention(organize_action);

    for (id, source, target) in connections {
        rocrate = rocrate.entity(
            Entity::new(id, "ParameterConnection")
                .reference("sourceParameter", source)
                .reference("targetParameter", target),
        );
    }

    Ok(rocrate.build())
}

/// [`build_crate`], plus the [`Validation`] against the profiles it was built for.
///
/// # Errors
/// See [`crate::provenance::ProvenanceError`].
pub fn build_validated(inputs: &CrateInputs) -> ProvenanceResult<(RoCrate, Validation)> {
    let crate_ = build_crate(inputs)?;
    let validation = crate_.validate();
    Ok((crate_, validation))
}

/// [`build_crate`], rejecting the result if it breaks a `Must` rule of a claimed profile.
///
/// # Errors
/// See [`crate::provenance::ProvenanceError`]; a non-conformant crate surfaces as
/// [`crate::provenance::ProvenanceError::Invalid`].
pub fn build_checked(inputs: &CrateInputs) -> ProvenanceResult<RoCrate> {
    let crate_ = build_crate(inputs)?;
    crate_.validate().into_result()?;
    Ok(crate_)
}

fn all_ports(graph: &WorkflowGraph) -> impl Iterator<Item = &PortNode> {
    graph
        .inputs
        .iter()
        .chain(&graph.outputs)
        .chain(&graph.tool_ports)
}

fn prefixed(wf: &str, id: &str) -> String {
    format!("{wf}{id}")
}

fn last_segment(id: &str) -> &str {
    id.rsplit('/').next().unwrap_or(id)
}

/// The image that actually ran a step, preferring what the run record says was pulled over what
/// the tool declares
fn actual_container_image(run: &RunRecord, step: &StepNode) -> Option<String> {
    run.steps
        .get(&step.id)
        .and_then(|s| s.container_image.clone())
        .or_else(|| step.container_image.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::config::WorkflowConfig;
    use crate::provenance::inputs::{Engine, StepRun};
    use chrono::{TimeZone, Utc};
    use commonwl::packed::PackedCWL;
    use std::{collections::BTreeSet, fs};

    fn fixture_packed() -> PackedCWL {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../testdata/rocrate/workflow.json"
        );
        let raw: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
        serde_json::from_value(raw["workflow"]["specification"].clone()).unwrap()
    }

    fn fixture_inputs() -> CrateInputs {
        let mut steps = HashMap::new();
        steps.insert(
            "#main/calculation".to_string(),
            StepRun {
                started_at: Some(Utc.with_ymd_and_hms(2026, 7, 23, 12, 21, 26).unwrap()),
                ended_at: Some(Utc.with_ymd_and_hms(2026, 7, 23, 12, 21, 30).unwrap()),
                container_image: Some("pandas/pandas:pip-all".to_string()),
            },
        );
        steps.insert(
            "#main/plot".to_string(),
            StepRun {
                started_at: Some(Utc.with_ymd_and_hms(2026, 7, 23, 12, 21, 31).unwrap()),
                ended_at: Some(Utc.with_ymd_and_hms(2026, 7, 23, 12, 21, 35).unwrap()),
                container_image: Some("user12398/pytest:v1.0.0".to_string()),
            },
        );

        CrateInputs::builder()
            .workflow(fixture_packed())
            .metadata(WorkflowConfig {
                name: "hello_s4n".to_string(),
                description: Some("some test workflow".to_string()),
                license: Some("https://spdx.org/licenses/CC-BY-4.0.html".to_string()),
                ..Default::default()
            })
            .run(RunRecord {
                engine: Engine {
                    name: "cwltool".to_string(),
                    version: Some("3.1.20210628163208".to_string()),
                },
                started_at: Some(Utc.with_ymd_and_hms(2026, 7, 23, 12, 21, 26).unwrap()),
                ended_at: Some(Utc.with_ymd_and_hms(2026, 7, 23, 12, 21, 35).unwrap()),
                steps,
            })
            .date_published(Utc.with_ymd_and_hms(2026, 7, 23, 12, 21, 26).unwrap())
            .build()
    }

    fn count_of_type(crate_: &RoCrate, type_name: &str) -> usize {
        crate_
            .graph
            .iter()
            .filter(|node| node.has_type(type_name))
            .count()
    }

    #[test]
    fn test_build_crate_is_conformant() {
        let crate_ = build_crate(&fixture_inputs()).unwrap();
        let validation = crate_.validate();
        assert!(
            validation.is_conformant(),
            "{:#?}",
            validation.errors().collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_build_crate_warnings_are_pinned() {
        let crate_ = build_crate(&fixture_inputs()).unwrap();
        let validation = crate_.validate();
        let warnings: BTreeSet<_> = validation.warnings().map(|v| v.rule).collect();
        assert_eq!(
            warnings,
            // No README.md is copied into the crate yet, and actions carry no `description`/
            // `agent` -- none of the data needed for those exists yet either. Known gaps, not
            // regressions; a NEW entry here should be treated as a real one.
            BTreeSet::from(["process::agent", "process::description", "wroc::readme"]),
            "a new SHOULD violation appeared -- update this allowlist if it's expected"
        );
    }

    #[test]
    fn test_build_crate_structure() {
        let crate_ = build_crate(&fixture_inputs()).unwrap();

        let main_entity = crate_.main_entity().unwrap();
        assert_eq!(main_entity.id, "workflow.json#main");
        assert!(main_entity.has_types(&[
            "File",
            "SoftwareSourceCode",
            "ComputationalWorkflow",
            "HowTo"
        ]));

        assert_eq!(count_of_type(&crate_, "ParameterConnection"), 4);
        assert_eq!(count_of_type(&crate_, "OrganizeAction"), 1);
        assert_eq!(count_of_type(&crate_, "ControlAction"), 2);
        // 2 steps + the workflow-level run.
        assert_eq!(count_of_type(&crate_, "CreateAction"), 3);
        assert_eq!(count_of_type(&crate_, "HowToStep"), 2);
        assert_eq!(count_of_type(&crate_, "ContainerImage"), 2);
        assert_eq!(count_of_type(&crate_, "SoftwareApplication"), 3); // 2 tools + the engine

        // population.csv, speakers_revised.csv, results.csv, results.svg -- plus the main
        // workflow entity itself, which is also `@type File`.
        assert_eq!(count_of_type(&crate_, "File"), 5);

        let root = crate_.root().unwrap();
        assert_eq!(root.iris("mentions").count(), 6); // 2 CreateAction + 2 ControlAction + workflow CreateAction + OrganizeAction
    }

    #[test]
    fn test_build_crate_no_workflow_errors() {
        let inputs = CrateInputs::builder()
            .workflow(PackedCWL::default())
            .metadata(WorkflowConfig::default())
            .run(RunRecord::default())
            .date_published(Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap())
            .build();
        let result = build_crate(&inputs);
        assert!(matches!(
            result,
            Err(crate::provenance::ProvenanceError::NoWorkflow)
        ));
    }
}
