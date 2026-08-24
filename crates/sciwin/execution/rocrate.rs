use rocrate::{RoCrate, profile::Profile, views};
use std::{io, path::Path};
use tracing::{debug, info};

use crate::execution::RunnerResult;

fn resolve_target(dir_or_zip: &Path) -> RunnerResult<()> {
    let ro_crate = if dir_or_zip.is_dir() {
        RoCrate::from_directory(dir_or_zip)?
    } else if dir_or_zip.extension() == Some(std::ffi::OsStr::new("zip")) {
        RoCrate::from_zip(dir_or_zip)?
    } else {
        return Err(super::RunnerError::RoCrateIO(
            rocrate::io::Error::Filesystem(io::Error::new(
                io::ErrorKind::InvalidData,
                "Not an archive or directory",
            )),
        ));
    };

    if !ro_crate.claims(&Profile::WorkflowRoCrate("1.0".into())) {
        return Err(super::RunnerError::Unknown(anyhow::anyhow!(
            "RO-Crate does not claim the Workflow RO-Crate profile"
        )));
    }

    let Some(wroc) = ro_crate.workflow() else {
        return Err(super::RunnerError::Unknown(anyhow::anyhow!(
            "RO-Crate does not contain any workflow"
        )));
    };
    debug!("RO Crate successfully identified as Workflow RO-Crate");

    if let Some(lang) = wroc.language() {
        info!("Language Found {lang:?}");
    }

    //if zip unzip

    //load and send resolved Run

    Ok(())
}
