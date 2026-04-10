# LCCP Specification

LCCP stands for **Lobster Configuration and Command-Line Parsing**.

This document defines a normative standard for a reusable library that provides both configuration resolution and command-line parsing for applications.

The standard is intentionally opinionated so that multiple applications using the library behave identically.

## 1. Scope

The library is initialized with a single required application name, referred to in this document as `app_name`.

The library is also initialized with the canonical absolute path of the running executable, referred to in this document as `executable_path`.

The library is responsible for:

- resolving configuration from a fixed set of sources
- parsing command-line arguments
- applying precedence rules consistently
- exposing resolved values through a query interface
- exposing provenance and override history for debugging
- emitting default and resolved configuration representations

## 2. Design Principles

### 2.1 Preferred configuration mechanisms

Environment variables are explicitly discouraged as a primary configuration mechanism.

Preferred mechanisms, in order of desirability, are:

1. configuration files
2. command-line configuration files
3. command-line key-value overrides
4. explicit named command-line arguments

Environment variables are supported only to ensure deterministic and portable behavior.

### 2.2 Canonical application identity

Every library instance operates on exactly one `app_name`.

`app_name`:

- must be provided by the application at library initialization time
- must be stable for the lifetime of the process
- must be used consistently in file names, environment variables, and generated output
- should consist only of lowercase ASCII letters, digits, and hyphens

Example:

`app_name = "example-app"`

## 3. Canonical key model

All configuration values are addressed by a canonical dot-separated key path.

Examples:

- `server.host`
- `server.port`
- `logging.level`
- `features.enable_cache`

This canonical key path is the source of truth for all mappings.

The library must map every configuration source into this canonical key space.

### 3.1 JSON and TOML mapping

Nested objects or tables map to dot-separated canonical keys.

Examples:

JSON:

```json
{
  "server": {
    "host": "localhost",
    "port": 8080
  }
}
```

TOML:

```toml
[server]
host = "localhost"
port = 8080
```

Both map to:

- `server.host`
- `server.port`

### 3.2 Command-line mapping

Explicit named command-line arguments map from kebab-case flag names to canonical dot-separated keys.

Examples:

- `--server-host` maps to `server.host`
- `--logging-level` maps to `logging.level`

### 3.3 Environment variable mapping

Environment variable names are derived mechanically from canonical keys.

Transformation rules:

1. start with `app_name`
2. convert to uppercase
3. replace hyphens with underscores
4. append an underscore
5. append the canonical key transformed to uppercase with dots replaced by underscores

Formula:

`ENV_NAME = UPPERCASE(app_name with '-' replaced by '_') + "_" + UPPERCASE(canonical key with '.' replaced by '_')`

Example:

- `app_name = "example-app"`
- canonical key = `server.host`
- environment variable = `EXAMPLE_APP_SERVER_HOST`

This mapping is mandatory and must not vary by application.

## 4. Configuration file formats

The library supports exactly two configuration file formats:

- JSON
- TOML

Selection is based on file extension:

- `.json` means JSON
- `.toml` means TOML

No other file format is supported.

### 4.1 Coexisting JSON and TOML files at the same discovered level

At each automatically discovered well-known configuration level, the library must look for both a JSON file and a TOML file.

If both files exist at the same level:

- both files must be loaded
- the library must emit a warning because this is unusual and discouraged
- the JSON file must be loaded first
- the TOML file must be loaded second

Therefore, at the same discovered level, TOML overrides JSON.

This rule applies only within a single discovered precedence level. It does not change the order between levels.

## 5. Precedence model

Configuration is resolved according to nine explicit precedence levels.

From lowest precedence to highest precedence, the levels are:

1. Level 1: library-provided defaults
2. Level 2: system configuration
3. Level 3: installation-relative configuration
4. Level 4: user configuration
5. Level 5: working directory configuration
6. Level 6: environment variables
7. Level 7: command-line configuration files
8. Level 8: generic command-line overrides
9. Level 9: explicit named command-line arguments

Within the same precedence level, values are applied from left to right.

For scalar values, the rightmost applied value wins.

## 6. Fixed configuration locations by precedence level

All file paths in this section are normative.

### 6.1 Level 1: library-provided defaults

Applications using the library must be able to register default values.

Defaults must be expressible in the same canonical key model used by all other sources.

### 6.2 Level 2: system configuration

The system configuration location consists of these candidate files, loaded in this order if present:

1. `/etc/<app_name>.json`
2. `/etc/<app_name>.toml`

This is the lowest-precedence discovered configuration level.

### 6.3 Level 3: installation-relative configuration

The installation-relative configuration location is resolved relative to `executable_path`, which must be supplied by the application when the library is initialized.

The executable directory is:

- `<binary_dir> = dirname(executable_path)`

The installation-relative configuration location consists of these candidate files, loaded in this order if present:

1. `<binary_dir>/../etc/<app_name>.json`
2. `<binary_dir>/../etc/<app_name>.toml`

This level exists so that multiple installed copies of the same binary can resolve different configuration based on their own installation roots.

Example:

If:

- `executable_path = "/opt/acme/example-app/bin/example-app"`

then the installation-relative configuration files are:

1. `/opt/acme/example-app/etc/example-app.json`
2. `/opt/acme/example-app/etc/example-app.toml`

### 6.4 Level 4: user configuration

The user configuration location consists of these candidate files, loaded in this order if present:

1. `~/.<app_name>/config.json`
2. `~/.<app_name>/config.toml`

This user-level directory is application-owned and may contain other application-specific files in addition to configuration.

### 6.5 Level 5: working directory configuration

The working directory configuration location consists of these candidate files, loaded in this order if present:

1. `./<app_name>.json`
2. `./<app_name>.toml`

This level is optional in the sense that no file may exist, but the discovery rule is fixed by the standard.

### 6.6 Level 6: environment variables

Environment variables are supported but discouraged.

They are applied after all automatically discovered configuration files and before all command-line-specified configuration input.

This makes environment variables stronger than discovered file-based defaults, but weaker than all explicit command-line-directed configuration.

### 6.7 Level 7: command-line configuration files

Additional configuration files may be specified on the command line using:

```sh
--config <path>
```

`--config` may appear more than once.

These files are processed from left to right.

The rightmost specified file has the highest precedence among command-line configuration files.

A command-line configuration file is loaded exactly as named. Its file extension determines whether it is parsed as JSON or TOML.

### 6.8 Level 8: generic command-line overrides

Generic key-value overrides are specified by repeated use of:

```sh
--set <key>=<value>
```

Rules:

- `<key>` must be a canonical dot-separated key
- `--set` may appear more than once
- `--set` values are processed from left to right
- the rightmost value wins for scalar values

Example:

```sh
--set server.host=example.internal --set logging.level=debug
```

### 6.9 Level 9: explicit named command-line arguments

Explicit named command-line arguments are the highest-precedence source.

Examples:

```sh
--server-host final.example.com
--logging-level info
```

If a value is supplied both through `--set` and through an explicit named argument, the explicit named argument wins.

## 7. Merge semantics

### 7.1 Scalars

Scalar values are replaced by the highest-precedence value.

This applies to:

- strings
- integers
- floats
- booleans
- null-equivalent values if supported by the host language

### 7.2 Objects and tables

Objects and tables are merged recursively by canonical key path.

A higher-precedence value replaces only the key or subtree it provides.

### 7.3 Arrays and repeated values

Arrays are not merged across precedence levels.

A higher-precedence array replaces a lower-precedence array in its entirety.

This avoids ambiguous behavior.

### 7.4 Repeated values at the same precedence level

When the same array-like option is supplied repeatedly at the same precedence level through command-line syntax intended for repetition, the resulting array is the ordered collection of those values.

Example:

```sh
--port 3 --port 4
```

resolves to:

- `port = [3, 4]`

This is treated as repeated input within one source level, not as array merging across levels.

## 8. Required library interface

The library must provide an interface that allows a consumer to:

- initialize the resolver with `app_name`
- initialize the resolver with `executable_path`
- declare the schema or set of supported options
- declare default values
- parse command-line arguments
- resolve final configuration values
- query values by canonical key
- inspect provenance for each resolved value
- emit default configuration
- emit resolved configuration
- emit debug resolution output

## 9. Command-line parsing responsibilities

The library is not only a configuration loader. It is also the command-line parsing facility for applications using this standard.

That means the library must support:

- declared named options
- repeated options
- typed values
- `--config`
- `--set`
- debug and inspection flags defined by the standard

Applications may extend the command line with additional arguments, but standard configuration behavior must remain unchanged.

## 10. Standard inspection and debugging behavior

### 10.1 Default configuration output

The library must support emitting the default configuration as a configuration document.

Required command:

```sh
--print-default-config
```

The output format must be TOML.

### 10.2 Resolved configuration output

The library must support emitting the final fully resolved configuration.

Required command:

```sh
--print-config
```

The output format must be TOML.

### 10.3 Resolution debug output

The library must support a debug view that includes provenance and override history.

Required command:

```sh
--debug-config
```

This output must include, for every resolved key:

- the final value
- the source that supplied the final value
- every lower-precedence source that also supplied the key
- the order in which overrides occurred

The output should make it possible to understand exactly how the final value was resolved.

### 10.4 Warning behavior for duplicate discovered formats

If both the JSON file and the TOML file exist at the same automatically discovered well-known level, the library must emit a warning identifying both paths and stating that TOML overrides JSON at that level.

This warning should not stop execution.

## 11. Provenance model

Every resolved key must retain provenance metadata.

At minimum, provenance must record:

- canonical key name
- final value
- source kind
- source location
- source precedence level
- raw source representation if available
- ordered override history

Valid source kinds include:

- default
- system_file
- install_file
- user_file
- working_dir_file
- environment
- cli_config_file
- cli_set
- cli_explicit

Examples of source location values:

- `/etc/example-app.json`
- `/etc/example-app.toml`
- `/opt/acme/example-app/etc/example-app.json`
- `/opt/acme/example-app/etc/example-app.toml`
- `~/.example-app/config.toml`
- `EXAMPLE_APP_SERVER_HOST`
- argument 7 of `argv`

## 12. Error handling

The library should fail deterministically.

The following conditions must produce clear errors:

- unreadable configuration files that were explicitly named with `--config`
- malformed JSON or TOML
- invalid `--set` syntax
- unknown explicit flags, unless the host application has declared them
- type conversion failures
- invalid canonical key names

Automatically discovered files may be absent without causing an error.

## 13. Recommended implementation note

Although environment variables are supported by this standard, applications should prefer configuration files and explicit command-line configuration.

Environment variables exist only to guarantee consistent behavior where they are unavoidable.
