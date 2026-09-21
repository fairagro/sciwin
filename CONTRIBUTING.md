# Contributing to SciWIn

Thanks for your interest in contributing to SciWIn Client (`s4n`)! This document covers how to set up your environment, the conventions we use, and how to get a change merged.

By participating in this project you agree to abide by our [Code of Conduct](CODE_OF_CONDUCT.md).

## Getting started

1. Install [Rust](https://www.rust-lang.org/tools/install) via `rustup`.
2. Clone the repository:
   ```bash
   git clone https://github.com/fairagro/sciwin
   cd sciwin
   ```
3. Build and run from source:
   ```bash
   cargo build
   cargo run -- <args>          # e.g. cargo run -- init -p demo
   ```

If you're using VS Code, install the [rust-analyzer](https://code.visualstudio.com/docs/languages/rust) extension.

### Cross-repo development

This repository depends on three sibling crates, each in its own FAIRagro repository: [`commonwl`](https://github.com/fairagro/commonwl), [`rocrate`](https://github.com/fairagro/ro-crate-lib), and [`reana`](https://github.com/fairagro/reana-cwl-client). By default these are pulled from crates.io, so a plain `cargo build` works without checking anything else out.

If your change spans repositories (e.g. a `commonwl` API change consumed here), clone the relevant repositories next to this one and patch them in via `[patch.crates-io]`, either in a workspace-level `.cargo/config.toml` above all the checkouts or directly in this repo's `Cargo.toml`. See the README's [Crate architecture & sibling repositories](README.md#crate-architecture--sibling-repositories) section for details.

## Project layout

- `crates/sciwin` (lib `sciwin`) - shared core logic for authoring, executing, and annotating workflows: `authoring/`, `execution/`, `provenance/`, `project/`, `repository/`.
- `crates/cli` (bin `s4n`) - the CLI entrypoint, a thin wrapper around `sciwin`; one file per subcommand under `commands/`.

Business logic belongs in `crates/sciwin` so it can be shared with [SciWIn-Studio](https://github.com/fairagro/sciwin_studio); `crates/cli` should stay thin.

## Making a change

1. Create a branch off `main`.
2. Make your change. Keep commits focused, write commit messages in the imperative mood (e.g. "fix container resolve", "add REANA retry logic"), and reference the relevant issue number where applicable.
3. Add or update tests for the behavior you changed.
4. Run the checks below locally before opening a pull request.
5. Open a pull request against `main` and describe what changed and why.

## Running checks locally

```bash
# Lint, matching CI (CI runs with RUSTFLAGS=-Dwarnings, so warnings fail the build)
cargo clippy --all-targets --all-features --workspace

# Run all unit and integration tests
cargo nextest run --workspace

# Run a single test
cargo test -p sciwin some_test_name

# Show stdout/stderr while testing
cargo nextest run --workspace --nocapture
```

CI runs clippy, a cross-platform build and test matrix, coverage (tarpaulin), and CWL conformance tests against the custom runner. A pull request needs to pass all of these before it can be merged.

`clippy.toml` disallows `std::println!`/`std::print!` workspace-wide; use `eprintln!`/`eprint!` instead, or add `#![allow(clippy::disallowed_macros)]` locally when stdout output is genuinely required (e.g. `-r`/`--raw` CLI output).

## Documentation

User-facing documentation lives under `docs/` (an Astro Starlight site, published at [fairagro.github.io/sciwin](https://fairagro.github.io/sciwin/)). If your change affects CLI behavior or workflows, update the relevant page alongside your code change.

## Reporting bugs and requesting features

Please use the issue templates:
- [Bug report](.github/ISSUE_TEMPLATE/bug_report.md) - include the version and operating system.
- [Feature request](.github/ISSUE_TEMPLATE/feature_request.md)

## License

By contributing, you agree that your contributions will be licensed under either the [MIT license](LICENSE-MIT) or the [Apache License 2.0](LICENSE-APACHE), at the user's option, same as the project itself.
