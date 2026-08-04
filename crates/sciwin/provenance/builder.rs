//! Turns a [`CrateInputs`] into an [`RoCrate`] -- pure: no filesystem, no network, no clock.
//! `CrateInputs::date_published` stands in for `Utc::now()`, which is what makes this
//! deterministic and its output byte-comparable across runs.
//!
//! [`build_crate`] is the assembly line: it walks the [`WorkflowGraph`] once, hands each concern
//! (parameters, files, containers, actions, ...) to its own builder function below, and folds
//! the resulting entities into the crate with the right role (`.entity()`/`.part()`/`.mention()`).
//! Each helper is plain data in, `Entity`/`Vec<Entity>` out -- none of them touch the `RoCrate`
//! builder itself, so they can be read and tested in isolation from the assembly order.

use std::collections::{BTreeMap, HashMap, HashSet};

use rocrate::{RoCrate, build::Entity, validate::Validation};

use crate::provenance::{
    ProvenanceResult,
    graph::{PortKind, PortNode, StepNode, WorkflowGraph},
    inputs::{CrateInputs, PayloadFile, RunRecord},
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

    let mut rocrate_builder = RoCrate::builder()
        .context(inputs.context.clone())
        .date_published(inputs.date_published.to_rfc3339())
        .name(inputs.metadata.name.clone())
        .maybe_description(inputs.metadata.description.clone())
        .maybe_license(inputs.metadata.license.clone());
    for profile in &inputs.profiles {
        rocrate_builder = rocrate_builder.conforms_to(profile.clone());
    }

    let cwl_version = inputs
        .workflow
        .cwl_version
        .clone()
        .unwrap_or_else(|| "v1.2".to_string());
    rocrate_builder = rocrate_builder.entity(cwl_language_entity(&cwl_version));

    for entity in formal_parameter_entities(wf, &graph) {
        rocrate_builder = rocrate_builder.entity(entity);
    }
    for entity in file_entities(wf, &graph, &inputs.payload) {
        rocrate_builder = rocrate_builder.part(entity);
    }

    let connections = connection_triples(wf, &graph);

    let (image_ids, image_entities) = container_images(run, &graph);
    for entity in image_entities {
        rocrate_builder = rocrate_builder.entity(entity);
    }

    rocrate_builder = rocrate_builder.entity(engine_entity(run));

    let tool_ids = distinct_tool_ids(&graph);
    for entity in tool_entities(wf, &graph, &tool_ids) {
        rocrate_builder = rocrate_builder.entity(entity);
    }

    let workflow_id = prefixed(wf, &graph.workflow_id);
    rocrate_builder = rocrate_builder.main_workflow(main_workflow_entity(
        wf,
        &workflow_id,
        &graph,
        &tool_ids,
        &connections,
    ));

    // Per-step HowToStep, CreateAction and ControlAction. `organize_objects` collects the
    // ControlAction ids, which is what OrganizeAction below points at.
    let mut organize_objects = Vec::new();
    for step in &graph.steps {
        let artifacts = step_entities(wf, &graph, run, &image_ids, &connections, step);
        rocrate_builder = rocrate_builder.entity(artifacts.how_to_step);
        rocrate_builder = rocrate_builder.mention(artifacts.create_action);
        rocrate_builder = rocrate_builder.mention(artifacts.control_action);
        organize_objects.push(artifacts.control_id);
    }

    let (workflow_create_action, workflow_create_id) =
        workflow_run_action(wf, &workflow_id, &graph, run);
    rocrate_builder = rocrate_builder.mention(workflow_create_action);

    // The engine's orchestration of the whole run. Always emitted, unlike the old generator,
    // which silently dropped it (and every File entity) when it could not regex an engine
    // version out of the logs.
    rocrate_builder =
        rocrate_builder.mention(organize_action(run, organize_objects, workflow_create_id));

    for entity in connection_entities(connections) {
        rocrate_builder = rocrate_builder.entity(entity);
    }

    Ok(rocrate_builder.build())
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

fn cwl_language_entity(cwl_version: &str) -> Entity {
    Entity::new(CWL_ID, "ComputerLanguage")
        .set("name", "Common Workflow Language")
        .set("alternateName", "CWL")
        .reference("identifier", "https://w3id.org/cwl/v1.2/")
        .reference("url", "https://www.commonwl.org/")
        .set("version", cwl_version.to_string())
}

/// The tools the workflow's steps run, in first-seen order, without duplicates.
fn distinct_tool_ids(graph: &WorkflowGraph) -> Vec<&str> {
    let mut ids: Vec<&str> = Vec::new();
    for step in &graph.steps {
        if !ids.contains(&step.run.as_str()) {
            ids.push(&step.run);
        }
    }
    ids
}

/// One `FormalParameter` per distinct port id
fn formal_parameter_entities(wf: &str, graph: &WorkflowGraph) -> Vec<Entity> {
    let mut seen: HashSet<&str> = HashSet::new();
    let mut entities = Vec::new();
    for port in all_ports(graph) {
        if !seen.insert(&port.id) {
            continue;
        }
        let mut entity = Entity::new(prefixed(wf, &port.id), "FormalParameter")
            .set("name", last_segment(&port.id));
        if let Some(ty) = port.additional_type {
            entity = entity.set("additionalType", ty.to_string());
        }
        entities.push(entity);
    }
    entities
}

/// One `File` per distinct crate-relative file name, referencing every `FormalParameter` it satisfies.
fn file_entities(wf: &str, graph: &WorkflowGraph, payload: &[PayloadFile]) -> Vec<Entity> {
    let mut file_parameters: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for port in all_ports(graph) {
        if let Some(file_name) = &port.file_name {
            file_parameters
                .entry(file_name.as_str())
                .or_default()
                .push(prefixed(wf, &port.id));
        }
    }

    file_parameters
        .into_iter()
        .map(|(file_name, parameter_ids)| {
            let matching_payload = payload.iter().find(|p| p.name == file_name);
            let mut entity = Entity::new(file_name, "File")
                .set("alternateName", file_name)
                .references("exampleOfWork", parameter_ids);
            if let Some(size) = matching_payload.and_then(|p| p.size) {
                entity = entity.set("contentSize", size);
            }
            if let Some(checksum) = matching_payload.and_then(|p| p.checksum.clone()) {
                entity = entity.set("sha1", checksum);
            }
            entity
        })
        .collect()
}

/// Connections, derived not basename-matched, given stable ids by position:  `(connection id, prefixed source, prefixed target)`.
fn connection_triples(wf: &str, graph: &WorkflowGraph) -> Vec<(String, String, String)> {
    graph
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
        .collect()
}

fn connection_entities(connections: Vec<(String, String, String)>) -> Vec<Entity> {
    connections
        .into_iter()
        .map(|(id, source, target)| {
            Entity::new(id, "ParameterConnection")
                .reference("sourceParameter", source)
                .reference("targetParameter", target)
        })
        .collect()
}

fn container_images(
    run: &RunRecord,
    graph: &WorkflowGraph,
) -> (HashMap<String, String>, Vec<Entity>) {
    let mut ids = HashMap::new();
    let mut entities = Vec::new();
    for step in &graph.steps {
        let Some(image) = actual_container_image(run, step) else {
            continue;
        };
        if ids.contains_key(&image) {
            continue;
        }
        let (name, tag) = image.split_once(':').unwrap_or((image.as_str(), "latest"));
        let id = format!("#image/{name}/{tag}");
        entities.push(
            Entity::new(id.clone(), "ContainerImage")
                .reference("additionalType", DOCKER_IMAGE_IRI)
                .set("name", name.to_string())
                .set("tag", tag.to_string())
                .set("registry", "docker.io"),
        );
        ids.insert(image, id);
    }
    (ids, entities)
}

fn engine_entity(run: &RunRecord) -> Entity {
    let mut entity =
        Entity::new(ENGINE_ID, "SoftwareApplication").set("name", run.engine.name.clone());
    if let Some(version) = &run.engine.version {
        entity = entity.set("softwareVersion", version.clone());
    }
    entity
}

fn tool_entities(wf: &str, graph: &WorkflowGraph, tool_ids: &[&str]) -> Vec<Entity> {
    tool_ids
        .iter()
        .map(|run_id| {
            let prefix = format!("{run_id}/");
            let inputs: Vec<String> = graph
                .tool_ports
                .iter()
                .filter(|p| p.kind == PortKind::Input && p.id.starts_with(&prefix))
                .map(|p| prefixed(wf, &p.id))
                .collect();
            let outputs: Vec<String> = graph
                .tool_ports
                .iter()
                .filter(|p| p.kind == PortKind::Output && p.id.starts_with(&prefix))
                .map(|p| prefixed(wf, &p.id))
                .collect();
            Entity::new(prefixed(wf, run_id), "SoftwareApplication")
                .set("name", run_id.trim_start_matches('#'))
                .references("input", inputs)
                .references("output", outputs)
        })
        .collect()
}

fn main_workflow_entity(
    wf: &str,
    workflow_id: &str,
    graph: &WorkflowGraph,
    tool_ids: &[&str],
    connections: &[(String, String, String)],
) -> Entity {
    let workflow_inputs: Vec<String> = graph.inputs.iter().map(|p| prefixed(wf, &p.id)).collect();
    let workflow_outputs: Vec<String> = graph.outputs.iter().map(|p| prefixed(wf, &p.id)).collect();
    let step_refs: Vec<String> = graph.steps.iter().map(|s| prefixed(wf, &s.id)).collect();
    let has_part: Vec<String> = tool_ids.iter().map(|id| prefixed(wf, id)).collect();

    let own_connections: Vec<String> = connections
        .iter()
        .filter(|(_, _, target)| workflow_outputs.contains(target))
        .map(|(id, _, _)| id.clone())
        .collect();

    Entity::new(
        workflow_id,
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
    .references("output", workflow_outputs)
    .references("step", step_refs)
    .references("connection", own_connections)
}

/// The entities one workflow step contributes: its `HowToStep`, the `CreateAction` recording its
/// run, and the `ControlAction` orchestrating it. `control_id` is handed back so the caller can
/// fold it into `OrganizeAction.object` without re-deriving it.
struct StepEntities {
    how_to_step: Entity,
    create_action: Entity,
    control_action: Entity,
    control_id: String,
}

fn step_entities(
    wf: &str,
    graph: &WorkflowGraph,
    run: &RunRecord,
    image_ids: &HashMap<String, String>,
    connections: &[(String, String, String)],
    step: &StepNode,
) -> StepEntities {
    let step_run = run.steps.get(&step.id);
    let step_prefix = format!("{}/", step.run);

    let target_prefix = format!("{wf}{step_prefix}");
    let step_connection_ids: Vec<String> = connections
        .iter()
        .filter(|(_, _, target)| target.starts_with(&target_prefix))
        .map(|(id, _, _)| id.clone())
        .collect();
    let how_to_step = Entity::new(prefixed(wf, &step.id), "HowToStep")
        .set("position", step.position as u64)
        .references("connection", step_connection_ids)
        .reference("workExample", prefixed(wf, &step.run));

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
    if let Some(image_id) = actual_container_image(run, step).and_then(|img| image_ids.get(&img)) {
        create_action = create_action.reference("containerImage", image_id.clone());
    }

    let control_id = format!("#orchestrate/{}", step.id.trim_start_matches('#'));
    let control_action = Entity::new(control_id.clone(), "ControlAction")
        .set(
            "name",
            format!("orchestrate {}", step.id.trim_start_matches('#')),
        )
        .reference("instrument", prefixed(wf, &step.id))
        .reference("object", create_id);

    StepEntities {
        how_to_step,
        create_action,
        control_action,
        control_id,
    }
}

/// The workflow-level `CreateAction`
fn workflow_run_action(
    wf: &str,
    workflow_id: &str,
    graph: &WorkflowGraph,
    run: &RunRecord,
) -> (Entity, String) {
    let object: Vec<String> = graph
        .inputs
        .iter()
        .filter_map(|p| p.file_name.clone())
        .collect();
    let result: Vec<String> = graph
        .outputs
        .iter()
        .filter_map(|p| p.file_name.clone())
        .collect();
    let id = format!("#run/{}", graph.workflow_id.trim_start_matches('#'));

    let mut action = Entity::new(id.clone(), "CreateAction")
        .set("name", format!("Run of {wf}"))
        .reference("instrument", workflow_id)
        .references("object", object)
        .references("result", result);
    if let Some(started) = run.started_at {
        action = action.set("startTime", started.to_rfc3339());
    }
    if let Some(ended) = run.ended_at {
        action = action.set("endTime", ended.to_rfc3339());
    }
    (action, id)
}

fn organize_action(run: &RunRecord, objects: Vec<String>, workflow_create_id: String) -> Entity {
    let name = match &run.engine.version {
        Some(version) => format!("Run of {} {version}", run.engine.name),
        None => format!("Run of {}", run.engine.name),
    };
    let mut action = Entity::new("#organize", "OrganizeAction")
        .set("name", name)
        .reference("instrument", ENGINE_ID)
        .references("object", objects)
        .references("result", vec![workflow_create_id]);
    if let Some(started) = run.started_at {
        action = action.set("startTime", started.to_rfc3339());
    }
    if let Some(ended) = run.ended_at {
        action = action.set("endTime", ended.to_rfc3339());
    }
    action
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
