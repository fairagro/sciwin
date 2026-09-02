//! Headless core library for SciWIn: authoring and running [CWL] workflows.
//!
//! Everything here is embeddable in any frontend -- a terminal app, a desktop GUI, a web
//! service. There is no printing, no prompting, and no credential storage; the library
//! reports through [`tracing`] and returns values, and the frontend decides how to show
//! them. See `CLAUDE.md` for the full rule.
//!
//! # The three pillars
//!
//! - [`authoring`] builds CWL documents. Its centerpiece is
//!   [`authoring::tool::create_tool`], which converts a shell command into a
//!   `CommandLineTool` -- optionally by running it once to see what it produces.
//! - [`execution`] runs CWL documents. [`execution::WorkflowRunner`] is one async interface
//!   over both [`execution::TaskRunner`] (local) and [`execution::ReanaRunner`] (remote),
//!   with status polling, log streaming and cancellation.
//! - [`repository`] is a thin git layer -- staging, committing, submodules -- shared by both.
//!
//! # Example
//!
//! Turn a command into a tool and write it into a project:
//!
//! ```no_run
//! use sciwin::authoring::tool::{ToolCreationOptions, create_tool};
//! use std::path::Path;
//!
//! # async fn example() -> sciwin::Result<()> {
//! let options = ToolCreationOptions::builder()
//!     .command(vec!["python3".to_string(), "script.py".to_string()])
//!     .save(true)
//!     .build();
//!
//! let tool = create_tool(Path::new("/path/to/project"), &options).await?;
//! println!("wrote {}", tool.path.display());
//! # Ok(())
//! # }
//! ```
//!
//! # Errors
//!
//! Each module has its own error type and result alias. [`Error`] unifies them for code that
//! spans modules, and `?` converts into it from any of the three.
//!
//! # Re-exports
//!
//! The two libraries this crate is built on are re-exported as modules rather than flattened
//! into the root, so their names stay attributable: [`cwl`] is [`commonwl`], [`reana`] is the
//! REANA client.
//!
//! [CWL]: https://www.commonwl.org/

pub mod cwl {
    //! Re-export of the [`commonwl`] crate: the CWL document model, parser and local engine.
    pub use commonwl::*; //add a prelude to commonwl lib
}
pub mod reana {
    //! Re-export of the `reana` crate: the REANA API client used by
    //! [`crate::execution::ReanaRunner`].
    pub use reana::*; //add a prelude to reana lib
}
pub mod rocrate {
    //! Re-export of the `rocrate` crate
    pub use rocrate::*; //add a prelude to rocrate lib
}

pub mod authoring;
pub mod container;
pub mod execution;
pub mod paths;
pub mod project;
pub mod provenance;
pub mod repository;
pub mod visualize;

mod error;
pub use error::{Error, Result};
