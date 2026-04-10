# Changelog

## Unreleased

## 0.3.0 - 2026-04-10
- Updated level 4 user configuration discovery to use `~/.<app_name>/config.{json,toml}` instead of `~/.config/<app_name>/...`.
- Updated vendored specification content to the upstream source commit that defines the new user-directory layout.
- Updated examples, tests, and reference output to use the application-owned hidden home-directory path.

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
