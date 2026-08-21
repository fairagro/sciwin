# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Overview

SciWIn (Scientific Workflow Infrastructure) helps researchers create, record, annotate, execute, and publish reproducible computational workflows as CWL (Common Workflow Language). It consists of two complementary tools built from one Cargo workspace:

- **SciWIn-Client** (`s4n`) — the command-line tool; primary and stable.
- **SciWIn-Studio** — a Dioxus-based desktop GUI wrapping the same core logic; currently in testing.

Crates (`crates/`):
- `sciwin` (lib `sciwin`) — the shared core logic used by both CLI and GUI: `authoring/` (generating CWL `CommandLineTool`/`Workflow` files, incl. `authoring/tool`), `execution/` (running workflows, `reana_runner.rs`/`task_runner.rs`, `reana_compat.rs` for REANA compatibility), `provenance/` (building provenance graphs — `builder.rs`, `graph.rs`, `inputs.rs`), `project/` (project/config handling), `repository/` (git plumbing: commits, `.ini`, submodules — most commands need the context of a git repo).
- `cli` (bin `s4n`) — the CLI entrypoint; `commands/` holds one file per subcommand (`create`, `connect`, `execute`, `init`, `list`, `packages`, `remove`, `save`, `visualize`), each calling into the `sciwin` crate.
- `gui` (bin `sciwin_studio`) — the SciWIn-Studio desktop app (Dioxus), `src/components` for UI, `js/` for embedded JS, `assets/` for static assets.

Sibling repos this project depends on / relates to: `commonwl` (`../commonwl`, CWL parsing + execution engine, imported here as the `commonwl` crate with `engine`/`tes` features), `rocrate` (`../ro-crate-lib`, RO-Crate support), and REANA execution support relates to `../reana-cwl-client`.

## Common commands

```bash
cargo build                          # build s4n (CLI)
cargo run -p s4n -- <args>           # run the CLI from source, e.g. cargo run -p s4n -- init -p demo
cargo nextest run --workspace        # run all unit + integration tests (or `bacon nextest` to watch)
cargo test -p sciwin some_test_name  # run a single test
cargo clippy --all-targets --all-features --workspace   # matches CI lint (CI runs with RUSTFLAGS=-Dwarnings)
```

SciWIn-Studio (GUI) requires the [Dioxus CLI](https://dioxuslabs.com/learn/0.7/getting_started/) (`dx`) plus GTK/WebKit system packages (see `.github/workflows/clippy.yml` for the exact apt package list on Linux):
```bash
dx serve -p sciwin_studio   # or: dx serve -p sciwin — launch SciWIn-Studio in dev mode
```

### CI

CI (`.github/workflows/ci.yml`) fans out to `clippy.yml` (`cargo clippy --all-targets --all-features --workspace`, warnings-as-errors), `build.yml` (build+test on Linux/macOS/Windows via `cargo nextest`, Windows uses podman instead of Docker), `tarpaulin.yml` (coverage), and `cwl.yml` (CWL conformance tests against the custom runner).

`clippy.toml` disallows `std::println!`/`std::print!` workspace-wide — use `eprintln!`/`eprint!`, or `#![allow(clippy::disallowed_macros)]` locally if stdout output is genuinely required (e.g. for `-r/--raw` CLI output).

## Architecture notes

- `s4n create <COMMAND> [ARGS]` runs the given command, observes its file inputs/outputs, and generates a CWL `CommandLineTool` from that — this "record by running" approach lives in `sciwin::authoring`.
- `s4n connect` wires CWL files into a `Workflow` by connecting named ports; special file identifiers `@inputs`/`@outputs` refer to the workflow's own top-level inputs/outputs.
- `s4n execute local` runs CWL files using SciWIn's own CWL runner (via the `commonwl` engine), not `cwltool`; conformance is not yet 1:1 with `cwltool`.
- Execution can also target REANA (`sciwin::execution::reana_runner`/`reana_compat`) as an alternative to local execution.
- Provenance tracking (`sciwin::provenance`) builds a provenance graph from executions, separate from the CWL authoring path.
- The CLI (`crates/cli`) and GUI (`crates/gui`) are thin front ends; business logic belongs in `crates/sciwin` so both share it.
