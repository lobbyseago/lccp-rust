# lccp-rust

Rust implementation of **Lobster Configuration and Command-Line Parsing (LCCP)**.

This crate implements the behavior defined in https://github.com/lobbyseago/lccp-spec.

## Version

Current crate version: `0.3.0`

The vendored upstream specification copy is stored under [`spec/upstream/`](./spec/upstream/) and versioned by upstream source commit because the specification repository does not yet publish release tags.

Level 4 user configuration is discovered from `~/.<app_name>/config.json` and `~/.<app_name>/config.toml`.

## API

A concise Rust-specific API specification is checked in at [`docs/api.md`](./docs/api.md).

That document covers:

- the resolver lifecycle
- configuration schema declaration
- user-level configuration discovery
- runtime-configurable app CLI options
- positional arguments and subcommands
- help rendering
- resolved command match access

## Example

A runnable example application and its input files are checked in under [`examples/`](./examples/).

Run it with:

```sh
cargo run --example example_app
```

That example:

- declares a small LCCP schema
- loads configuration from fixed discovered locations
- applies environment and CLI overrides
- prints default config, resolved config, warnings, and provenance debug output

The checked-in example inputs live in [`examples/example-app/`](./examples/example-app/), and a reference output is stored in [`examples/example-app/output.txt`](./examples/example-app/output.txt).
