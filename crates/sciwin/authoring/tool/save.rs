//! Post-processing, serializing and writing a finished tool.

use crate::{
    authoring::{
        AuthoringError, AuthoringResult,
        tool::{paths::resolve_path, postprocess::post_process_cwl},
    },
    paths::TrustedPathExt,
    repository::{self, Repository},
};
use anyhow::Context as _;
use commonwl::{
    documents::{CWLDocument, CommandLineTool},
    format::format_cwl,
    inputs::DefaultValue,
    requirements::{ListingItems, StringOrInclude, ToolRequirements, WorkDirItems},
};
use std::{fs, path::Path};

/// Post-processes, rewires and serializes `cwl` as it would be saved at `path`.
pub(super) fn finalize_tool(cwl: &mut CommandLineTool, path: &Path) -> AuthoringResult<String> {
    post_process_cwl(cwl)?;
    let yaml = prepare_save(cwl, path)?;
    format_cwl(&yaml)
        .map_err(|e| AuthoringError::Unknown(anyhow::anyhow!("Failed to format CWL: {e}")))
}

/// Writes `yaml` to `path`, which is interpreted relative to `project_root`.
pub(super) fn save_tool_to_disk(
    yaml: &str,
    project_root: &Path,
    path: &Path,
    repo: &Repository,
    commit: bool,
) -> AuthoringResult<()> {
    let target = project_root.join_trusted_unchecked(path)?;

    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create directories for {}", parent.display()))?;
    }
    fs::write(&target, yaml)
        .with_context(|| format!("Creation of file {} failed", target.display()))?;

    if commit {
        repository::stage_file(repo, &target)?;
        //the message names the tool's place in the project, not on this machine
        repository::commit(repo, &format!("🪄 Creation of `{}`", path.display()))?;
    }
    Ok(())
}

/// Every path in the document is written relative to wherever the document itself lands, so
/// they all have to be rebased on `path` before serializing.
fn prepare_save(tool: &mut CommandLineTool, path: &Path) -> AuthoringResult<String> {
    //rewire paths to new location
    for input in &mut tool.inputs {
        if let Some(DefaultValue::FileOrDirectory(fod)) = &mut input.default {
            fod.set_location(Some(resolve_path(fod.location().as_ref().unwrap(), path)));
        }
    }

    if let Some(requirements) = &mut tool.requirements {
        for requirement in requirements {
            if let ToolRequirements::DockerRequirement(docker) = requirement {
                if let Some(StringOrInclude::Include(include)) = &mut docker.docker_file {
                    include.include = resolve_path(&include.include, path);
                }
            } else if let ToolRequirements::InitialWorkDirRequirement(iwdr) = requirement
                && let WorkDirItems::ListingItems(listing) = &mut iwdr.listing
            {
                for item in listing {
                    if let ListingItems::Dirent(dirent) = item
                        && let StringOrInclude::Include(include) = &mut dirent.entry
                    {
                        include.include = resolve_path(&include.include, path);
                    }
                }
            }
        }
    }
    Ok(serde_saphyr::to_string(&CWLDocument::CommandLineTool(
        tool.clone(),
    ))?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use commonwl::{
        OneOrMany,
        files::{Dirent, File, FileOrDirectory},
        inputs::{CommandInputParameter, CommandLineBinding, DefaultValue},
        requirements::{
            DockerRequirement, Include, InitialWorkDirRequirement, ListingItems, StringOrInclude,
            WorkDirItems,
        },
        types::CWLType,
    };
    use serde_json::Value;
    use std::path::MAIN_SEPARATOR_STR;

    #[test]
    pub fn test_cwl_save() {
        fn os_path(s: &str) -> String {
            s.split('/').collect::<Vec<_>>().join(MAIN_SEPARATOR_STR)
        }
        let inputs = vec![
            CommandInputParameter::builder()
                .id("positional1")
                .default(DefaultValue::FileOrDirectory(FileOrDirectory::File(
                    File::builder().location("testdata/input.txt").build(),
                )))
                .r#type(CWLType::String)
                .input_binding(CommandLineBinding::builder().position(0).build())
                .build(),
            CommandInputParameter::builder()
                .id("option1")
                .r#type(CWLType::String)
                .input_binding(CommandLineBinding::builder().prefix("--option1").build())
                .default(DefaultValue::Any(Value::String("value1".to_string())))
                .build(),
        ];
        let mut clt = CommandLineTool::builder()
            .base_command(OneOrMany::Many(vec![
                "python3".to_string(),
                "test/script.py".to_string(),
            ]))
            .inputs(inputs)
            .requirements(vec![
                ToolRequirements::InitialWorkDirRequirement(InitialWorkDirRequirement {
                    listing: WorkDirItems::ListingItems(vec![ListingItems::Dirent(
                        Dirent::builder()
                            .entry(StringOrInclude::Include(
                                Include::builder()
                                    .include(os_path("test/script.py"))
                                    .build(),
                            ))
                            .entryname("test/script.py".to_string())
                            .build(),
                    )]),
                }),
                ToolRequirements::DockerRequirement(
                    DockerRequirement::builder()
                        .docker_file(StringOrInclude::Include(
                            Include::builder()
                                .include(os_path("test/data/Dockerfile"))
                                .build(),
                        ))
                        .docker_image_id("test")
                        .build(),
                ),
            ])
            .build();

        prepare_save(&mut clt, Path::new("workflows/tool/tool.cwl")).unwrap();

        //check if paths are rewritten upon tool saving

        assert_eq!(
            clt.inputs[0].default,
            Some(DefaultValue::FileOrDirectory(FileOrDirectory::File(
                File::builder()
                    .location(os_path("../../testdata/input.txt"))
                    .build()
            )))
        );
        let requirements = clt.requirements.as_ref().unwrap();
        let req_0 = &requirements[0];
        let req_1 = &requirements[1];
        assert_eq!(
            *req_0,
            ToolRequirements::InitialWorkDirRequirement(InitialWorkDirRequirement {
                listing: WorkDirItems::ListingItems(vec![ListingItems::Dirent(
                    Dirent::builder()
                        .entry(StringOrInclude::Include(
                            Include::builder()
                                .include(os_path("../../test/script.py"))
                                .build()
                        ))
                        .entryname("test/script.py".to_string())
                        .build()
                )]),
            })
        );
        assert_eq!(
            *req_1,
            ToolRequirements::DockerRequirement(
                DockerRequirement::builder()
                    .docker_file(StringOrInclude::Include(
                        Include::builder()
                            .include(os_path("../../test/data/Dockerfile"))
                            .build()
                    ))
                    .docker_image_id("test".to_string())
                    .build()
            )
        );
    }
}
