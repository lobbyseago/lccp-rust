use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use lccp::{
    CliOptionSpec, CommandSpec, LccpError, OptionSpec, PositionalSpec, ResolveOptions, Resolver,
    SourceKind, ValueType,
};
use serde_json::{json, Value};
use tempfile::TempDir;

struct Harness {
    _temp: TempDir,
    system_dir: PathBuf,
    home_dir: PathBuf,
    cwd: PathBuf,
    executable_path: PathBuf,
}

impl Harness {
    fn new() -> Self {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        let system_dir = root.join("system");
        let home_dir = root.join("home");
        let cwd = root.join("cwd");
        let install_root = root.join("install-root");
        let executable_path = install_root.join("bin").join("example-app");

        fs::create_dir_all(&system_dir).unwrap();
        fs::create_dir_all(home_dir.join(".example-app")).unwrap();
        fs::create_dir_all(&cwd).unwrap();
        fs::create_dir_all(install_root.join("bin")).unwrap();
        fs::create_dir_all(install_root.join("etc")).unwrap();
        fs::write(&executable_path, "").unwrap();

        Self {
            _temp: temp,
            system_dir,
            home_dir,
            cwd,
            executable_path,
        }
    }

    fn resolve_options(&self) -> ResolveOptions {
        ResolveOptions {
            cwd: self.cwd.clone(),
            home_dir: Some(self.home_dir.clone()),
            env: BTreeMap::new(),
            system_config_dir: self.system_dir.clone(),
        }
    }
}

fn resolver(executable_path: &Path) -> Resolver {
    let mut resolver = Resolver::new("example-app", executable_path).unwrap();
    resolver
        .declare_option(
            OptionSpec::new("server.host", ValueType::String)
                .unwrap()
                .with_help("Set the server host"),
        )
        .unwrap();
    resolver
        .declare_option(OptionSpec::new("server.port", ValueType::Integer).unwrap())
        .unwrap();
    resolver
        .declare_option(OptionSpec::new("logging.level", ValueType::String).unwrap())
        .unwrap();
    resolver
        .declare_option(OptionSpec::new("features.enable_cache", ValueType::Boolean).unwrap())
        .unwrap();
    resolver
        .declare_option(OptionSpec::repeated("ports", ValueType::Integer).unwrap())
        .unwrap();

    resolver
        .set_default("server.host", json!("default.example"))
        .unwrap();
    resolver.set_default("server.port", json!(80)).unwrap();
    resolver
        .set_default("logging.level", json!("warn"))
        .unwrap();
    resolver
        .set_default("features.enable_cache", json!(false))
        .unwrap();
    resolver.set_default("ports", json!([1, 2])).unwrap();

    resolver
}

fn write_file(path: impl AsRef<Path>, content: &str) {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, content).unwrap();
}

#[test]
fn canonical_mappings_match_the_spec() {
    let harness = Harness::new();
    let resolver = resolver(&harness.executable_path);

    assert_eq!(
        resolver.environment_variable_name("server.host").unwrap(),
        "EXAMPLE_APP_SERVER_HOST"
    );
    assert_eq!(
        OptionSpec::new("features.enable_cache", ValueType::Boolean)
            .unwrap()
            .long_flag(),
        "features-enable-cache"
    );
}

#[test]
fn full_precedence_chain_is_applied_in_order() {
    let harness = Harness::new();
    let resolver = resolver(&harness.executable_path);
    let mut options = harness.resolve_options();

    write_file(
        harness.system_dir.join("example-app.json"),
        r#"{"server":{"host":"system.example"}}"#,
    );
    write_file(
        harness
            .executable_path
            .parent()
            .unwrap()
            .join("../etc/example-app.toml"),
        r#"[server]
host = "install.example"
"#,
    );
    write_file(
        harness.home_dir.join(".example-app/config.toml"),
        r#"[server]
host = "user.example"
"#,
    );
    write_file(
        harness.cwd.join("example-app.json"),
        r#"{"server":{"host":"cwd.example"}}"#,
    );
    let cli_config = harness.cwd.join("cli-config.toml");
    write_file(
        &cli_config,
        r#"[server]
host = "cli-config.example"
"#,
    );
    options.env.insert(
        "EXAMPLE_APP_SERVER_HOST".to_string(),
        "env.example".to_string(),
    );

    let resolved = resolver
        .resolve_with(
            [
                "example-app",
                "--config",
                "cli-config.toml",
                "--set",
                "server.host=set.example",
                "--server-host",
                "explicit.example",
            ],
            options,
        )
        .unwrap();

    assert_eq!(resolved.get("server.host"), Some(json!("explicit.example")));

    let provenance = resolved.provenance("server.host").unwrap();
    let kinds: Vec<SourceKind> = provenance
        .history
        .iter()
        .map(|record| record.source.kind)
        .collect();
    assert_eq!(
        kinds,
        vec![
            SourceKind::Default,
            SourceKind::SystemFile,
            SourceKind::InstallFile,
            SourceKind::UserFile,
            SourceKind::WorkingDirFile,
            SourceKind::Environment,
            SourceKind::CliConfigFile,
            SourceKind::CliSet,
            SourceKind::CliExplicit,
        ]
    );
}

#[test]
fn discovered_json_and_toml_warn_and_toml_wins_at_the_same_level() {
    let harness = Harness::new();
    let resolver = resolver(&harness.executable_path);

    write_file(
        harness.system_dir.join("example-app.json"),
        r#"{"logging":{"level":"json-level"}}"#,
    );
    write_file(
        harness.system_dir.join("example-app.toml"),
        r#"[logging]
level = "toml-level"
"#,
    );

    let resolved = resolver
        .resolve_with(["example-app"], harness.resolve_options())
        .unwrap();

    assert_eq!(resolved.get("logging.level"), Some(json!("toml-level")));
    assert_eq!(resolved.warnings().len(), 1);
    assert!(resolved.warnings()[0].contains("example-app.json"));
    assert!(resolved.warnings()[0].contains("example-app.toml"));
}

#[test]
fn install_relative_configuration_uses_the_executable_path() {
    let harness = Harness::new();
    let resolver = resolver(&harness.executable_path);

    write_file(
        harness
            .executable_path
            .parent()
            .unwrap()
            .join("../etc/example-app.json"),
        r#"{"logging":{"level":"install-only"}}"#,
    );

    let resolved = resolver
        .resolve_with(["example-app"], harness.resolve_options())
        .unwrap();

    assert_eq!(resolved.get("logging.level"), Some(json!("install-only")));
}

#[test]
fn object_merge_is_recursive_and_querying_parent_keys_returns_subtrees() {
    let harness = Harness::new();
    let resolver = resolver(&harness.executable_path);

    write_file(
        harness.home_dir.join(".example-app/config.toml"),
        r#"[server]
host = "merged.example"
"#,
    );

    let resolved = resolver
        .resolve_with(["example-app"], harness.resolve_options())
        .unwrap();

    assert_eq!(resolved.get("server.host"), Some(json!("merged.example")));
    assert_eq!(resolved.get("server.port"), Some(json!(80)));
    assert_eq!(
        resolved.get("server"),
        Some(json!({"host":"merged.example","port":80}))
    );
}

#[test]
fn arrays_replace_across_levels_and_repeated_cli_values_collect_in_order() {
    let harness = Harness::new();
    let resolver = resolver(&harness.executable_path);

    write_file(harness.cwd.join("example-app.toml"), r#"ports = [3, 4]"#);

    let resolved = resolver
        .resolve_with(
            ["example-app", "--ports", "7", "--ports", "8"],
            harness.resolve_options(),
        )
        .unwrap();

    assert_eq!(resolved.get("ports"), Some(json!([7, 8])));
    let provenance = resolved.provenance("ports").unwrap();
    assert_eq!(provenance.history.len(), 3);
    assert_eq!(provenance.history[2].source.kind, SourceKind::CliExplicit);
}

#[test]
fn print_flags_and_toml_emitters_are_available() {
    let harness = Harness::new();
    let resolver = resolver(&harness.executable_path);

    let resolved = resolver
        .resolve_with(
            [
                "example-app",
                "--print-default-config",
                "--print-config",
                "--debug-config",
            ],
            harness.resolve_options(),
        )
        .unwrap();

    assert!(resolved.standard_flags().print_default_config);
    assert!(resolved.standard_flags().print_config);
    assert!(resolved.standard_flags().debug_config);

    let default_toml = resolver.emit_default_config().unwrap();
    assert!(default_toml.contains("host = \"default.example\""));
    assert!(default_toml.contains("level = \"warn\""));

    let resolved_toml = resolved.emit_toml().unwrap();
    assert!(resolved_toml.contains("port = 80"));
    assert!(resolved_toml.contains("enable_cache = false"));
}

#[test]
fn debug_output_includes_final_source_and_override_history() {
    let harness = Harness::new();
    let resolver = resolver(&harness.executable_path);
    let mut options = harness.resolve_options();
    options.env.insert(
        "EXAMPLE_APP_SERVER_HOST".to_string(),
        "env.example".to_string(),
    );

    let resolved = resolver
        .resolve_with(
            ["example-app", "--server-host", "explicit.example"],
            options,
        )
        .unwrap();

    let debug = resolved.debug_output();
    assert!(debug.contains("server.host = \"explicit.example\""));
    assert!(debug.contains("final_source: cli_explicit"));
    assert!(debug.contains("1. default"));
    assert!(debug.contains("2. environment"));
    assert!(debug.contains("3. cli_explicit"));
}

#[test]
fn cli_config_files_are_loaded_left_to_right() {
    let harness = Harness::new();
    let resolver = resolver(&harness.executable_path);

    write_file(
        harness.cwd.join("one.toml"),
        r#"[logging]
level = "one"
"#,
    );
    write_file(
        harness.cwd.join("two.toml"),
        r#"[logging]
level = "two"
"#,
    );

    let resolved = resolver
        .resolve_with(
            [
                "example-app",
                "--config",
                "one.toml",
                "--config",
                "two.toml",
            ],
            harness.resolve_options(),
        )
        .unwrap();

    assert_eq!(resolved.get("logging.level"), Some(json!("two")));
}

#[test]
fn cli_sets_are_loaded_left_to_right_and_explicit_flags_win_over_them() {
    let harness = Harness::new();
    let resolver = resolver(&harness.executable_path);

    let resolved = resolver
        .resolve_with(
            [
                "example-app",
                "--set",
                "logging.level=info",
                "--set",
                "logging.level=debug",
                "--logging-level",
                "error",
            ],
            harness.resolve_options(),
        )
        .unwrap();

    assert_eq!(resolved.get("logging.level"), Some(json!("error")));
    let provenance = resolved.provenance("logging.level").unwrap();
    let kinds: Vec<SourceKind> = provenance
        .history
        .iter()
        .map(|record| record.source.kind)
        .collect();
    assert_eq!(
        kinds,
        vec![
            SourceKind::Default,
            SourceKind::CliSet,
            SourceKind::CliSet,
            SourceKind::CliExplicit,
        ]
    );
}

#[test]
fn absent_discovered_files_do_not_cause_errors() {
    let harness = Harness::new();
    let resolver = resolver(&harness.executable_path);

    let resolved = resolver
        .resolve_with(["example-app"], harness.resolve_options())
        .unwrap();

    assert_eq!(resolved.get("server.host"), Some(json!("default.example")));
    assert!(resolved.warnings().is_empty());
}

#[test]
fn invalid_set_unknown_flags_and_type_conversion_fail_clearly() {
    let harness = Harness::new();
    let resolver = resolver(&harness.executable_path);

    let invalid_set = resolver
        .resolve_with(
            ["example-app", "--set", "server.host"],
            harness.resolve_options(),
        )
        .unwrap_err();
    assert!(matches!(invalid_set, LccpError::InvalidSetSyntax(_)));

    let unknown_flag = resolver
        .resolve_with(
            ["example-app", "--unknown-flag", "x"],
            harness.resolve_options(),
        )
        .unwrap_err();
    assert!(matches!(unknown_flag, LccpError::UnknownFlag(_)));

    let type_error = resolver
        .resolve_with(
            ["example-app", "--server-port", "nope"],
            harness.resolve_options(),
        )
        .unwrap_err();
    assert!(matches!(type_error, LccpError::TypeConversion { .. }));
}

#[test]
fn malformed_explicit_config_files_and_unsupported_formats_fail() {
    let harness = Harness::new();
    let resolver = resolver(&harness.executable_path);

    write_file(harness.cwd.join("broken.json"), "{not-json");
    write_file(harness.cwd.join("broken.toml"), "server = {");
    write_file(harness.cwd.join("config.yaml"), "server:\n  host: nope\n");
    fs::create_dir_all(harness.cwd.join("unreadable.toml")).unwrap();

    let json_error = resolver
        .resolve_with(
            ["example-app", "--config", "broken.json"],
            harness.resolve_options(),
        )
        .unwrap_err();
    assert!(matches!(json_error, LccpError::ParseJsonConfig { .. }));

    let toml_error = resolver
        .resolve_with(
            ["example-app", "--config", "broken.toml"],
            harness.resolve_options(),
        )
        .unwrap_err();
    assert!(matches!(toml_error, LccpError::ParseTomlConfig { .. }));

    let format_error = resolver
        .resolve_with(
            ["example-app", "--config", "config.yaml"],
            harness.resolve_options(),
        )
        .unwrap_err();
    assert!(matches!(
        format_error,
        LccpError::UnsupportedConfigFormat(_)
    ));

    let read_error = resolver
        .resolve_with(
            ["example-app", "--config", "unreadable.toml"],
            harness.resolve_options(),
        )
        .unwrap_err();
    assert!(matches!(read_error, LccpError::ReadConfigFile { .. }));
}

#[test]
fn invalid_app_names_and_keys_are_rejected() {
    assert!(matches!(
        Resolver::new("Example-App", "/tmp/example-app"),
        Err(LccpError::InvalidAppName(_))
    ));
    assert!(matches!(
        OptionSpec::new("Server.Host", ValueType::String),
        Err(LccpError::InvalidCanonicalKey(_))
    ));

    let harness = Harness::new();
    let mut resolver = Resolver::new("example-app", &harness.executable_path).unwrap();
    let error = resolver
        .set_default("bad-key", Value::String("x".to_string()))
        .unwrap_err();
    assert!(matches!(error, LccpError::InvalidCanonicalKey(_)));
}

#[test]
fn app_defined_options_and_positionals_are_parsed_by_the_library() {
    let harness = Harness::new();
    let mut resolver = resolver(&harness.executable_path);
    resolver
        .command_mut()
        .add_option(
            CliOptionSpec::new("verbose", "verbose", ValueType::Boolean)
                .unwrap()
                .with_help("Enable verbose logging"),
        )
        .unwrap();
    resolver
        .command_mut()
        .add_positional(
            PositionalSpec::new("input", ValueType::String)
                .unwrap()
                .with_help("Input file"),
        )
        .unwrap();

    let resolved = resolver
        .resolve_with(
            ["example-app", "--verbose", "input.txt"],
            harness.resolve_options(),
        )
        .unwrap();

    assert_eq!(
        resolved.command_matches().option("verbose").cloned(),
        Some(json!(true))
    );
    assert_eq!(
        resolved.command_matches().positional("input").cloned(),
        Some(json!("input.txt"))
    );
}

#[test]
fn subcommands_and_their_positionals_are_parsed_without_a_second_parser() {
    let harness = Harness::new();
    let mut resolver = resolver(&harness.executable_path);

    let mut serve = CommandSpec::new("serve")
        .unwrap()
        .with_help("Run the server");
    serve
        .add_option(
            CliOptionSpec::new("daemonize", "daemonize", ValueType::Boolean)
                .unwrap()
                .with_help("Run in the background"),
        )
        .unwrap();
    serve
        .add_positional(
            PositionalSpec::new("profile", ValueType::String)
                .unwrap()
                .with_help("Profile name"),
        )
        .unwrap();
    resolver.command_mut().add_subcommand(serve).unwrap();

    let resolved = resolver
        .resolve_with(
            ["example-app", "serve", "prod", "--daemonize"],
            harness.resolve_options(),
        )
        .unwrap();

    let subcommand = resolved.command_matches().subcommand().unwrap();
    assert_eq!(subcommand.command_name(), "serve");
    assert_eq!(
        subcommand.positional("profile").cloned(),
        Some(json!("prod"))
    );
    assert_eq!(subcommand.option("daemonize").cloned(), Some(json!(true)));
}

#[test]
fn help_text_includes_config_options_app_options_positionals_and_subcommands() {
    let harness = Harness::new();
    let mut resolver = resolver(&harness.executable_path);
    resolver
        .command_mut()
        .add_option(
            CliOptionSpec::new("verbose", "verbose", ValueType::Boolean)
                .unwrap()
                .with_help("Enable verbose logging"),
        )
        .unwrap();
    resolver
        .command_mut()
        .add_positional(
            PositionalSpec::new("input", ValueType::String)
                .unwrap()
                .optional()
                .with_help("Optional input file"),
        )
        .unwrap();
    resolver
        .command_mut()
        .add_subcommand(
            CommandSpec::new("serve")
                .unwrap()
                .with_help("Run the server"),
        )
        .unwrap();

    let root_help = resolver.render_help();
    assert!(root_help.contains("Standard Options:"));
    assert!(root_help.contains("Configuration Options:"));
    assert!(root_help.contains("--server-host"));
    assert!(root_help.contains("Set the server host"));
    assert!(root_help.contains("Options:"));
    assert!(root_help.contains("--verbose"));
    assert!(root_help.contains("Positionals:"));
    assert!(root_help.contains("input"));
    assert!(root_help.contains("Subcommands:"));
    assert!(root_help.contains("serve"));

    let serve_help = resolver.render_help_for(["serve"]).unwrap();
    assert!(serve_help.contains("Usage: example-app serve"));
    assert!(serve_help.contains("Run the server"));
}

#[test]
fn unknown_app_flags_and_help_requests_are_reported_by_the_command_model() {
    let harness = Harness::new();
    let mut resolver = resolver(&harness.executable_path);

    let mut serve = CommandSpec::new("serve").unwrap();
    serve
        .add_option(
            CliOptionSpec::new("daemonize", "daemonize", ValueType::Boolean)
                .unwrap()
                .with_help("Run in the background"),
        )
        .unwrap();
    resolver.command_mut().add_subcommand(serve).unwrap();

    let error = resolver
        .resolve_with(["example-app", "--unknown"], harness.resolve_options())
        .unwrap_err();
    assert!(matches!(error, LccpError::UnknownFlag(_)));

    let resolved = resolver
        .resolve_with(
            ["example-app", "serve", "--help"],
            harness.resolve_options(),
        )
        .unwrap();
    assert_eq!(
        resolved.command_matches().help_command_path(),
        Some(vec!["example-app".to_string(), "serve".to_string()])
    );
}
