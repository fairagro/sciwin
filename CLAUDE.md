# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Overview

SciWIn (Scientific Workflow Infrastructure) helps researchers create, record, annotate, execute, and publish reproducible computational workflows as CWL (Common Workflow Language). This repository is SciWIn-Client (`s4n`), the command-line tool; primary and stable. SciWIn-Studio, a Tauri v2 + SvelteKit desktop GUI wrapping the same core logic, lives in the separate sibling repo `../sciwin_studio` (see its own `CLAUDE.md`), not in this Cargo workspace.

Crates (`crates/`):
- `sciwin` (lib `sciwin`) — the shared core logic used by both the CLI and SciWIn-Studio: `authoring/` (generating CWL `CommandLineTool`/`Workflow` files, incl. `authoring/tool`), `execution/` (running workflows, `reana_runner.rs`/`task_runner.rs`, `reana_compat.rs` for REANA compatibility), `provenance/` (building provenance graphs — `builder.rs`, `graph.rs`, `inputs.rs`), `project/` (project/config handling), `repository/` (git plumbing: commits, `.ini`, submodules — most commands need the context of a git repo).
- `cli` (bin `s4n`) — the CLI entrypoint; `commands/` holds one file per subcommand (`create`, `connect`, `execute`, `init`, `list`, `packages`, `remove`, `save`, `visualize`), each calling into the `sciwin` crate.

Sibling repos this project depends on / relates to: `commonwl` (`../commonwl`, CWL parsing + execution engine, imported here as the `commonwl` crate with `engine`/`tes` features), `rocrate` (`../ro-crate-lib`, RO-Crate support), REANA execution support relates to `../reana-cwl-client`, and `../sciwin_studio` depends on this repo's `sciwin` crate.

## Common commands

```bash
cargo build                          # build s4n (CLI)
cargo run -p s4n -- <args>           # run the CLI from source, e.g. cargo run -p s4n -- init -p demo
cargo nextest run --workspace        # run all unit + integration tests (or `bacon nextest` to watch)
cargo test -p sciwin some_test_name  # run a single test
cargo clippy --all-targets --all-features --workspace   # matches CI lint (CI runs with RUSTFLAGS=-Dwarnings)
```

SciWIn-Studio is built and run from the `../sciwin_studio` repo (see its own `CLAUDE.md`), not from here.

### CI

CI (`.github/workflows/ci.yml`) fans out to `clippy.yml` (`cargo clippy --all-targets --all-features --workspace`, warnings-as-errors), `build.yml` (build+test on Linux/macOS/Windows via `cargo nextest`, Windows uses podman instead of Docker), `tarpaulin.yml` (coverage), and `cwl.yml` (CWL conformance tests against the custom runner).

`clippy.toml` disallows `std::println!`/`std::print!` workspace-wide — use `eprintln!`/`eprint!`, or `#![allow(clippy::disallowed_macros)]` locally if stdout output is genuinely required (e.g. for `-r/--raw` CLI output).

## Architecture notes

- `s4n create <COMMAND> [ARGS]` runs the given command, observes its file inputs/outputs, and generates a CWL `CommandLineTool` from that — this "record by running" approach lives in `sciwin::authoring`.
- `s4n connect` wires CWL files into a `Workflow` by connecting named ports; special file identifiers `@inputs`/`@outputs` refer to the workflow's own top-level inputs/outputs.
- `s4n execute local` runs CWL files using SciWIn's own CWL runner (via the `commonwl` engine), not `cwltool`; conformance is not yet 1:1 with `cwltool`.
- Execution can also target REANA (`sciwin::execution::reana_runner`/`reana_compat`) as an alternative to local execution.
- Provenance tracking (`sciwin::provenance`) builds a provenance graph from executions, separate from the CWL authoring path.
- The CLI (`crates/cli`) and SciWIn-Studio (`../sciwin_studio`) are thin front ends; business logic belongs in `crates/sciwin` so both share it.
