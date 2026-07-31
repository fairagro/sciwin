//! Staging files into the tool's working directory via `InitialWorkDirRequirement`.
//!
//! A CWL tool runs in an isolated directory, so anything it reads by a relative path -- its
//! own script, a fixed data file -- has to be declared as staged or it won't be there.

use commonwl::{
    documents::CommandLineTool,
    files::Dirent,
    requirements::{
        InitialWorkDirRequirement, ListingItems, StringOrInclude, ToolRequirements, WorkDirItems,
    },
};
use std::path::Path;

use crate::authoring::tool::parser::sanitize_id;

/// A requirement staging `path` verbatim, or `None` when it isn't an existing file.
///
/// `path` is checked relative to `base` (the project root)
pub(super) fn iwdr_for_existing_file(path: &str, base: &Path) -> Option<ToolRequirements> {
    base.join(path).is_file().then(|| {
        ToolRequirements::InitialWorkDirRequirement(
            InitialWorkDirRequirement::new_from_filename_include(path),
        )
    })
}

/// Stages a fixed File input into the working dir, merging into an existing
/// `InitialWorkDirRequirement` rather than replacing it.
pub(super) fn stage_fixed_input(tool: &mut CommandLineTool, input: &str) {
    let dirent = Dirent::builder()
        .entry(StringOrInclude::String(entry_name(input)))
        .entryname(input)
        .build();

    let Some(req) = tool.get_requirement_mut::<InitialWorkDirRequirement>() else {
        tool.append_requirement_mut(ToolRequirements::InitialWorkDirRequirement(
            InitialWorkDirRequirement {
                listing: WorkDirItems::ListingItems(vec![ListingItems::Dirent(dirent)]),
            },
        ));
        return;
    };

    match &mut req.listing {
        WorkDirItems::Expression(expr) => {
            req.listing = WorkDirItems::ListingItems(vec![
                ListingItems::Dirent(dirent),
                ListingItems::Expression(expr.clone()),
            ]);
        }
        WorkDirItems::ListingItems(items) => items.push(ListingItems::Dirent(dirent)),
    }
}

/// The CWL expression pulling a staged file's content from the input of the same name.
fn entry_name(input: &str) -> String {
    let name = sanitize_id(input);
    format!("$(inputs.{name})")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    pub fn test_entry_name_numeric_only() {
        // an all-numeric filename must not produce the invalid "$(inputs.)"
        assert_eq!(entry_name("123"), "$(inputs.123)");
    }
}
