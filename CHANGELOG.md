# Changelog

## Unreleased

## 0.2.0 - 2026-04-06
- Added a runtime-configurable application command model so consumers can declare app-specific flags, positionals, subcommands, and help text inside `lccp`.
- Added parsed command match access on resolved results.
- Added help rendering for standard LCCP flags, config options, app options, positionals, and subcommands.
- Added a Rust-specific API document under `docs/api.md`.

## 0.1.0 - 2026-04-05
- Initial public release of the Rust LCCP implementation.
- Added a resolver API with schema declaration, fixed-source discovery, precedence handling, provenance tracking, TOML emission, and debug output.
- Vendored the upstream LCCP specification by source commit under `spec/upstream/`.
- Added conformance-focused integration tests covering precedence, merge rules, warnings, and error cases.
- Added a runnable example app with checked-in input files and reference output.
