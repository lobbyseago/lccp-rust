use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use lccp::{OptionSpec, ResolveOptions, Resolver, ValueType};
use serde_json::json;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    std::env::set_current_dir(env!("CARGO_MANIFEST_DIR"))?;

    let root = PathBuf::from("examples/example-app");
    let mut resolver = Resolver::new("example-app", root.join("install-root/bin/example-app"))?;
    resolver.declare_option(OptionSpec::new("server.host", ValueType::String)?)?;
    resolver.declare_option(OptionSpec::new("server.port", ValueType::Integer)?)?;
    resolver.declare_option(OptionSpec::new("logging.level", ValueType::String)?)?;
    resolver.declare_option(OptionSpec::new(
        "features.enable_cache",
        ValueType::Boolean,
    )?)?;
    resolver.declare_option(OptionSpec::repeated("ports", ValueType::Integer)?)?;

    resolver.set_default("server.host", json!("default.example"))?;
    resolver.set_default("server.port", json!(80))?;
    resolver.set_default("logging.level", json!("warn"))?;
    resolver.set_default("features.enable_cache", json!(false))?;
    resolver.set_default("ports", json!([1, 2]))?;

    let mut env = BTreeMap::new();
    env.insert(
        "EXAMPLE_APP_SERVER_HOST".to_string(),
        "env.example".to_string(),
    );

    let argv = [
        "example-app",
        "--config",
        "cli-config.toml",
        "--set",
        "server.host=set.example",
        "--server-host",
        "explicit.example",
        "--ports",
        "7",
        "--ports",
        "8",
    ];

    let resolved = resolver.resolve_with(
        argv,
        ResolveOptions {
            cwd: root.join("cwd"),
            home_dir: Some(root.join("home")),
            env,
            system_config_dir: root.join("system"),
        },
    )?;

    println!("Input Files:");
    print_file(&root.join("system/example-app.json"))?;
    print_file(&root.join("system/example-app.toml"))?;
    print_file(&root.join("install-root/etc/example-app.toml"))?;
    print_file(&root.join("home/.config/example-app/config.toml"))?;
    print_file(&root.join("cwd/example-app.json"))?;
    print_file(&root.join("cwd/cli-config.toml"))?;

    println!("Environment:");
    println!("EXAMPLE_APP_SERVER_HOST=env.example");
    println!();

    println!("CLI:");
    println!(
        "{}",
        argv.iter()
            .map(|part| part.to_string())
            .collect::<Vec<_>>()
            .join(" ")
    );
    println!();

    println!("Warnings:");
    for warning in resolved.warnings() {
        println!("- {warning}");
    }
    println!();

    println!("Default Config:");
    print!("{}", resolver.emit_default_config()?);
    println!();

    println!("Resolved Config:");
    print!("{}", resolved.emit_toml()?);
    println!();

    println!("Debug Output:");
    print!("{}", resolved.debug_output());

    Ok(())
}

fn print_file(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    println!("{}", path.display());
    print!("{}", fs::read_to_string(path)?);
    println!();
    Ok(())
}
