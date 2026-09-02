# sciwin
[![🦀 Continuous Integration](https://github.com/fairagro/sciwin/actions/workflows/ci.yml/badge.svg)](https://github.com/fairagro/sciwin/actions/workflows/ci.yml) ![Crates.io License](https://img.shields.io/crates/l/sciwin)
![Crates.io Version](https://img.shields.io/crates/v/sciwin) ![Crates.io Total Downloads](https://img.shields.io/crates/d/sciwin)

Headless core library for [SciWIn](https://github.com/fairagro/sciwin): authoring and running [CWL](https://www.commonwl.org/) workflows.

## Overview
`sciwin` is the shared logic behind **SciWIn-Client** (`s4n`), the command-line tool, and **SciWIn-Studio**, the desktop GUI. Everything here is embeddable in any frontend — a terminal app, a desktop GUI, a web service. There is no printing, no prompting, and no credential storage; the library reports through [`tracing`](https://docs.rs/tracing) and returns values, and the frontend decides how to show them.

### The three pillars
- [`authoring`](https://docs.rs/sciwin/latest/sciwin/authoring/) builds CWL documents. Its centerpiece is `authoring::tool::create_tool`, which converts a shell command into a `CommandLineTool` — optionally by running it once to see what it produces.
- [`execution`](https://docs.rs/sciwin/latest/sciwin/execution/) runs CWL documents. `execution::WorkflowRunner` is one async interface over both `execution::TaskRunner` (local) and `execution::ReanaRunner` (remote), with status polling, log streaming and cancellation.
- [`repository`](https://docs.rs/sciwin/latest/sciwin/repository/) is a thin git layer — staging, committing, submodules — shared by both.

Besides these, the crate also provides `project` (project/config handling), `provenance` (building provenance graphs from executions) and `container` (container image resolution).

## Installation
```bash
cargo add sciwin
```

## Usage
Turn a command into a tool and write it into a project:
```rust,no_run
use sciwin::authoring::tool::{ToolCreationOptions, create_tool};
use std::path::Path;

async fn example() -> sciwin::Result<()> {
    let options = ToolCreationOptions::builder()
        .command(vec!["python3".to_string(), "script.py".to_string()])
        .save(true)
        .build();

    let tool = create_tool(Path::new("/path/to/project"), &options).await?;
    println!("wrote {}", tool.path.display());
    Ok(())
}
```

### Errors
Each module has its own error type and result alias. `sciwin::Error` unifies them for code that spans modules, and `?` converts into it from any of the three.

### Re-exports
`commonwl` and `reana` are re-exported as `sciwin::cwl` and `sciwin::reana` rather than flattened into the root, so their names stay attributable.

## Related crates
`sciwin` builds on three sibling FAIRagro libraries, each maintained in its own repository and published as its own crate:

| Crate | Repository | Purpose |
|--|--|--|
| [`commonwl`](https://crates.io/crates/commonwl) | [fairagro/commonwl](https://github.com/fairagro/commonwl) | Parses CWL documents and provides the execution engine (local, Docker and TES backends) |
| [`rocrate`](https://crates.io/crates/rocrate) | [fairagro/ro-crate-lib](https://github.com/fairagro/ro-crate-lib) | Reads, writes, builds and validates [RO-Crates](https://www.researchobject.org/ro-crate/), including Workflow RO-Crates and Workflow Run Crates |
| [`reana`](https://crates.io/crates/reana) | [fairagro/reana-cwl-client](https://github.com/fairagro/reana-cwl-client) | Client for [REANA](https://reanahub.io/), used as an alternative execution backend to local runs |

If you're looking for a ready-to-use CLI rather than a library, see the [`s4n` command-line tool](https://github.com/fairagro/sciwin) built on top of this crate.

## License
Licensed under either of [Apache License, Version 2.0](https://github.com/fairagro/sciwin/blob/main/LICENSE-APACHE) or
[MIT license](https://github.com/fairagro/sciwin/blob/main/LICENSE-MIT) at your option.
