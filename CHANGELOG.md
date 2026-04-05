# Changelog

## Unreleased

## 0.1.0 - 2026-04-05
- Initial public release of the Rust LCCP implementation.
- Added a resolver API with schema declaration, fixed-source discovery, precedence handling, provenance tracking, TOML emission, and debug output.
- Vendored the upstream LCCP specification by source commit under `spec/upstream/`.
- Added conformance-focused integration tests covering precedence, merge rules, warnings, and error cases.
- Added a runnable example app with checked-in input files and reference output.
