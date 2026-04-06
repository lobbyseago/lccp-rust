//! Rust implementation of Lobster Configuration and Command-Line Parsing (LCCP).
//!
//! This crate combines two concerns in one parser:
//!
//! - LCCP configuration resolution across defaults, files, environment, and CLI overrides
//! - application-specific command-line parsing for flags, positionals, subcommands, and help text
//!
//! The main entry point is [`Resolver`].
//!
//! Typical usage:
//!
//! 1. construct a [`Resolver`]
//! 2. declare LCCP configuration options with [`OptionSpec`]
//! 3. register default values
//! 4. configure app CLI structure through [`CommandSpec`], [`CliOptionSpec`], and [`PositionalSpec`]
//! 5. call [`Resolver::resolve`] or [`Resolver::resolve_with`]
//! 6. read resolved config plus parsed command matches from [`ResolvedConfig`]
//!
//! A concise Rust-facing API guide is also checked in at `docs/api.md` in the repository.
//!
use std::collections::BTreeMap;
use std::env;
use std::fmt;
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde_json::{Map as JsonMap, Number, Value};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValueType {
    String,
    Integer,
    Float,
    Boolean,
    Array(Box<ValueType>),
}

impl ValueType {
    fn description(&self) -> String {
        match self {
            Self::String => "string".to_string(),
            Self::Integer => "integer".to_string(),
            Self::Float => "float".to_string(),
            Self::Boolean => "boolean".to_string(),
            Self::Array(inner) => format!("array<{}>", inner.description()),
        }
    }

    fn repeated_element_type(&self) -> Option<&ValueType> {
        match self {
            Self::Array(inner) => Some(inner.as_ref()),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptionSpec {
    key: String,
    value_type: ValueType,
    repeated_cli: bool,
    long_flag: String,
    help: Option<String>,
    value_name: Option<String>,
}

impl OptionSpec {
    pub fn new(key: impl Into<String>, value_type: ValueType) -> Result<Self, LccpError> {
        let key = key.into();
        validate_canonical_key(&key)?;

        Ok(Self {
            long_flag: canonical_key_to_flag(&key),
            key,
            value_type,
            repeated_cli: false,
            help: None,
            value_name: None,
        })
    }

    pub fn repeated(key: impl Into<String>, element_type: ValueType) -> Result<Self, LccpError> {
        let key = key.into();
        validate_canonical_key(&key)?;

        Ok(Self {
            long_flag: canonical_key_to_flag(&key),
            key,
            value_type: ValueType::Array(Box::new(element_type)),
            repeated_cli: true,
            help: None,
            value_name: None,
        })
    }

    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }

    pub fn with_value_name(mut self, value_name: impl Into<String>) -> Self {
        self.value_name = Some(value_name.into());
        self
    }

    pub fn key(&self) -> &str {
        &self.key
    }

    pub fn value_type(&self) -> &ValueType {
        &self.value_type
    }

    pub fn long_flag(&self) -> &str {
        &self.long_flag
    }

    pub fn repeated_cli(&self) -> bool {
        self.repeated_cli
    }

    pub fn help(&self) -> Option<&str> {
        self.help.as_deref()
    }

    pub fn value_name(&self) -> Option<&str> {
        self.value_name.as_deref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliOptionSpec {
    name: String,
    long_flag: String,
    value_type: ValueType,
    repeated: bool,
    required: bool,
    help: Option<String>,
    value_name: Option<String>,
}

impl CliOptionSpec {
    pub fn new(
        name: impl Into<String>,
        long_flag: impl Into<String>,
        value_type: ValueType,
    ) -> Result<Self, LccpError> {
        let name = name.into();
        let long_flag = long_flag.into();
        validate_argument_name(&name)?;
        validate_long_flag(&long_flag)?;

        Ok(Self {
            name,
            long_flag,
            value_type,
            repeated: false,
            required: false,
            help: None,
            value_name: None,
        })
    }

    pub fn repeated(
        name: impl Into<String>,
        long_flag: impl Into<String>,
        element_type: ValueType,
    ) -> Result<Self, LccpError> {
        let name = name.into();
        let long_flag = long_flag.into();
        validate_argument_name(&name)?;
        validate_long_flag(&long_flag)?;

        Ok(Self {
            name,
            long_flag,
            value_type: ValueType::Array(Box::new(element_type)),
            repeated: true,
            required: false,
            help: None,
            value_name: None,
        })
    }

    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }

    pub fn with_value_name(mut self, value_name: impl Into<String>) -> Self {
        self.value_name = Some(value_name.into());
        self
    }

    pub fn required(mut self, required: bool) -> Self {
        self.required = required;
        self
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn long_flag(&self) -> &str {
        &self.long_flag
    }

    pub fn value_type(&self) -> &ValueType {
        &self.value_type
    }

    pub fn repeated_values(&self) -> bool {
        self.repeated
    }

    pub fn required_option(&self) -> bool {
        self.required
    }

    pub fn help(&self) -> Option<&str> {
        self.help.as_deref()
    }

    pub fn value_name(&self) -> Option<&str> {
        self.value_name.as_deref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PositionalSpec {
    name: String,
    value_type: ValueType,
    required: bool,
    multiple: bool,
    help: Option<String>,
}

impl PositionalSpec {
    pub fn new(name: impl Into<String>, value_type: ValueType) -> Result<Self, LccpError> {
        let name = name.into();
        validate_argument_name(&name)?;

        Ok(Self {
            name,
            value_type,
            required: true,
            multiple: false,
            help: None,
        })
    }

    pub fn optional(mut self) -> Self {
        self.required = false;
        self
    }

    pub fn multiple(mut self) -> Self {
        self.multiple = true;
        self
    }

    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn value_type(&self) -> &ValueType {
        &self.value_type
    }

    pub fn required(&self) -> bool {
        self.required
    }

    pub fn multiple_values(&self) -> bool {
        self.multiple
    }

    pub fn help(&self) -> Option<&str> {
        self.help.as_deref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpec {
    name: String,
    help: Option<String>,
    options: BTreeMap<String, CliOptionSpec>,
    flag_index: BTreeMap<String, String>,
    positionals: Vec<PositionalSpec>,
    subcommands: BTreeMap<String, CommandSpec>,
}

impl CommandSpec {
    pub fn new(name: impl Into<String>) -> Result<Self, LccpError> {
        let name = name.into();
        validate_command_name(&name)?;

        Ok(Self {
            name,
            help: None,
            options: BTreeMap::new(),
            flag_index: BTreeMap::new(),
            positionals: Vec::new(),
            subcommands: BTreeMap::new(),
        })
    }

    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }

    pub fn add_option(&mut self, spec: CliOptionSpec) -> Result<(), LccpError> {
        if self.options.contains_key(spec.name()) {
            return Err(LccpError::DuplicateCliOption(spec.name().to_string()));
        }
        if self.flag_index.contains_key(spec.long_flag()) {
            return Err(LccpError::DuplicateFlag(spec.long_flag().to_string()));
        }

        self.flag_index
            .insert(spec.long_flag().to_string(), spec.name().to_string());
        self.options.insert(spec.name().to_string(), spec);
        Ok(())
    }

    pub fn add_positional(&mut self, spec: PositionalSpec) -> Result<(), LccpError> {
        if self
            .positionals
            .iter()
            .any(|existing| existing.name() == spec.name())
        {
            return Err(LccpError::DuplicatePositional(spec.name().to_string()));
        }
        if self.positionals.iter().any(PositionalSpec::multiple_values) {
            return Err(LccpError::InvalidPositionalLayout(self.name.clone()));
        }
        self.positionals.push(spec);
        Ok(())
    }

    pub fn add_subcommand(&mut self, spec: CommandSpec) -> Result<(), LccpError> {
        if self.subcommands.contains_key(&spec.name) {
            return Err(LccpError::DuplicateSubcommand(spec.name.clone()));
        }
        self.subcommands.insert(spec.name.clone(), spec);
        Ok(())
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn help(&self) -> Option<&str> {
        self.help.as_deref()
    }

    pub fn options(&self) -> &BTreeMap<String, CliOptionSpec> {
        &self.options
    }

    pub fn positionals(&self) -> &[PositionalSpec] {
        &self.positionals
    }

    pub fn subcommands(&self) -> &BTreeMap<String, CommandSpec> {
        &self.subcommands
    }

    pub fn subcommand(&self, name: &str) -> Option<&CommandSpec> {
        self.subcommands.get(name)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CommandMatches {
    command_name: String,
    options: BTreeMap<String, Value>,
    positionals: BTreeMap<String, Value>,
    subcommand: Option<Box<CommandMatches>>,
    help_requested: bool,
}

impl CommandMatches {
    fn new(command_name: impl Into<String>) -> Self {
        Self {
            command_name: command_name.into(),
            options: BTreeMap::new(),
            positionals: BTreeMap::new(),
            subcommand: None,
            help_requested: false,
        }
    }

    pub fn command_name(&self) -> &str {
        &self.command_name
    }

    pub fn option(&self, name: &str) -> Option<&Value> {
        self.options.get(name)
    }

    pub fn positional(&self, name: &str) -> Option<&Value> {
        self.positionals.get(name)
    }

    pub fn options(&self) -> &BTreeMap<String, Value> {
        &self.options
    }

    pub fn positionals(&self) -> &BTreeMap<String, Value> {
        &self.positionals
    }

    pub fn subcommand(&self) -> Option<&CommandMatches> {
        self.subcommand.as_deref()
    }

    pub fn help_requested(&self) -> bool {
        self.help_requested
    }

    pub fn help_command_path(&self) -> Option<Vec<String>> {
        if self.help_requested {
            return Some(vec![self.command_name.clone()]);
        }

        let nested = self.subcommand.as_deref()?.help_command_path()?;
        let mut path = vec![self.command_name.clone()];
        path.extend(nested);
        Some(path)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    Default,
    SystemFile,
    InstallFile,
    UserFile,
    WorkingDirFile,
    Environment,
    CliConfigFile,
    CliSet,
    CliExplicit,
}

impl fmt::Display for SourceKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Default => write!(f, "default"),
            Self::SystemFile => write!(f, "system_file"),
            Self::InstallFile => write!(f, "install_file"),
            Self::UserFile => write!(f, "user_file"),
            Self::WorkingDirFile => write!(f, "working_dir_file"),
            Self::Environment => write!(f, "environment"),
            Self::CliConfigFile => write!(f, "cli_config_file"),
            Self::CliSet => write!(f, "cli_set"),
            Self::CliExplicit => write!(f, "cli_explicit"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SourceInfo {
    pub kind: SourceKind,
    pub location: String,
    pub precedence_level: u8,
    pub raw: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OverrideRecord {
    pub value: Value,
    pub source: SourceInfo,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Provenance {
    pub key: String,
    pub history: Vec<OverrideRecord>,
}

impl Provenance {
    pub fn final_value(&self) -> Option<&Value> {
        self.history.last().map(|record| &record.value)
    }

    pub fn final_source(&self) -> Option<&SourceInfo> {
        self.history.last().map(|record| &record.source)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StandardFlags {
    pub print_default_config: bool,
    pub print_config: bool,
    pub debug_config: bool,
}

#[derive(Debug, Clone)]
struct ResolvedEntry {
    value: Value,
    history: Vec<OverrideRecord>,
}

#[derive(Debug, Clone)]
pub struct ResolvedConfig {
    entries: BTreeMap<String, ResolvedEntry>,
    warnings: Vec<String>,
    standard_flags: StandardFlags,
    command_matches: CommandMatches,
}

impl ResolvedConfig {
    pub fn get(&self, key: &str) -> Option<Value> {
        if let Some(entry) = self.entries.get(key) {
            return Some(entry.value.clone());
        }

        let prefix = format!("{key}.");
        let mut matching = self
            .entries
            .iter()
            .filter(|(candidate, _)| candidate.starts_with(&prefix))
            .peekable();

        matching.peek()?;

        let mut root = Value::Object(JsonMap::new());
        for (candidate, entry) in matching {
            let suffix = &candidate[prefix.len()..];
            insert_value_at_path(&mut root, suffix, entry.value.clone());
        }

        Some(root)
    }

    pub fn provenance(&self, key: &str) -> Option<Provenance> {
        self.entries.get(key).map(|entry| Provenance {
            key: key.to_string(),
            history: entry.history.clone(),
        })
    }

    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    pub fn standard_flags(&self) -> &StandardFlags {
        &self.standard_flags
    }

    pub fn command_matches(&self) -> &CommandMatches {
        &self.command_matches
    }

    pub fn emit_toml(&self) -> Result<String, LccpError> {
        emit_flat_map_as_toml(
            self.entries
                .iter()
                .map(|(key, entry)| (key.as_str(), &entry.value)),
        )
    }

    pub fn debug_output(&self) -> String {
        let mut out = String::new();

        for (key, entry) in &self.entries {
            let final_record = entry
                .history
                .last()
                .expect("resolved entries always have at least one history item");
            out.push_str(&format!("{key} = {}\n", render_value(&entry.value)));
            out.push_str(&format!(
                "  final_source: {} @ {} (level {})\n",
                final_record.source.kind,
                final_record.source.location,
                final_record.source.precedence_level
            ));
            out.push_str("  override_history:\n");

            for (index, record) in entry.history.iter().enumerate() {
                out.push_str(&format!(
                    "    {}. {} @ {} => {}\n",
                    index + 1,
                    record.source.kind,
                    record.source.location,
                    render_value(&record.value)
                ));
            }
        }

        out
    }

    fn insert_warning(&mut self, warning: String) {
        self.warnings.push(warning);
    }

    fn apply_leaf(&mut self, key: &str, value: Value, source: SourceInfo) {
        remove_descendants(&mut self.entries, key);
        remove_ancestors(&mut self.entries, key);

        match self.entries.get_mut(key) {
            Some(entry) => {
                entry.value = value.clone();
                entry.history.push(OverrideRecord { value, source });
            }
            None => {
                self.entries.insert(
                    key.to_string(),
                    ResolvedEntry {
                        value: value.clone(),
                        history: vec![OverrideRecord { value, source }],
                    },
                );
            }
        }
    }
}

impl ResolvedConfig {
    fn empty(command_name: impl Into<String>) -> Self {
        Self {
            entries: BTreeMap::new(),
            warnings: Vec::new(),
            standard_flags: StandardFlags::default(),
            command_matches: CommandMatches::new(command_name),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ResolveOptions {
    pub cwd: PathBuf,
    pub home_dir: Option<PathBuf>,
    pub env: BTreeMap<String, String>,
    pub system_config_dir: PathBuf,
}

impl Default for ResolveOptions {
    fn default() -> Self {
        Self {
            cwd: env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            home_dir: env::var_os("HOME").map(PathBuf::from),
            env: env::vars().collect(),
            system_config_dir: PathBuf::from("/etc"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Resolver {
    app_name: String,
    executable_path: PathBuf,
    schema: BTreeMap<String, OptionSpec>,
    flag_index: BTreeMap<String, String>,
    defaults: BTreeMap<String, Value>,
    root_command: CommandSpec,
}

impl Resolver {
    pub fn new(
        app_name: impl Into<String>,
        executable_path: impl Into<PathBuf>,
    ) -> Result<Self, LccpError> {
        let app_name = app_name.into();
        validate_app_name(&app_name)?;

        Ok(Self {
            root_command: CommandSpec::new(app_name.clone())?,
            app_name,
            executable_path: executable_path.into(),
            schema: BTreeMap::new(),
            flag_index: BTreeMap::new(),
            defaults: BTreeMap::new(),
        })
    }

    pub fn declare_option(&mut self, spec: OptionSpec) -> Result<(), LccpError> {
        if self.schema.contains_key(spec.key()) {
            return Err(LccpError::DuplicateOption(spec.key().to_string()));
        }

        if self.flag_index.contains_key(spec.long_flag()) {
            return Err(LccpError::DuplicateFlag(spec.long_flag().to_string()));
        }

        self.flag_index
            .insert(spec.long_flag().to_string(), spec.key().to_string());
        self.schema.insert(spec.key().to_string(), spec);
        Ok(())
    }

    pub fn set_default(&mut self, key: impl Into<String>, value: Value) -> Result<(), LccpError> {
        let key = key.into();
        validate_canonical_key(&key)?;
        let validated = self.normalize_declared_value(&key, value)?;
        self.defaults.insert(key, validated);
        Ok(())
    }

    pub fn environment_variable_name(&self, key: &str) -> Result<String, LccpError> {
        validate_canonical_key(key)?;

        Ok(format!(
            "{}_{}",
            self.app_name.replace('-', "_").to_ascii_uppercase(),
            key.replace('.', "_").to_ascii_uppercase()
        ))
    }

    pub fn emit_default_config(&self) -> Result<String, LccpError> {
        emit_flat_map_as_toml(
            self.defaults
                .iter()
                .map(|(key, value)| (key.as_str(), value)),
        )
    }

    pub fn command(&self) -> &CommandSpec {
        &self.root_command
    }

    pub fn command_mut(&mut self) -> &mut CommandSpec {
        &mut self.root_command
    }

    pub fn render_help(&self) -> String {
        self.render_help_for::<&str, Vec<&str>>(Vec::new())
            .expect("root command always exists")
    }

    pub fn render_help_for<S, I>(&self, path: I) -> Result<String, LccpError>
    where
        S: AsRef<str>,
        I: IntoIterator<Item = S>,
    {
        let path: Vec<String> = path
            .into_iter()
            .map(|item| item.as_ref().to_string())
            .collect();
        let command = self.command_for_path(&path)?;
        Ok(render_help_text(
            &self.app_name,
            command,
            &path,
            self.schema.values().collect(),
        ))
    }

    pub fn resolve<I, S>(&self, argv: I) -> Result<ResolvedConfig, LccpError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.resolve_with(argv, ResolveOptions::default())
    }

    pub fn resolve_with<I, S>(
        &self,
        argv: I,
        options: ResolveOptions,
    ) -> Result<ResolvedConfig, LccpError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let parsed = self.parse_argv(argv)?;
        let mut resolved = ResolvedConfig::empty(self.root_command.name.clone());
        resolved.standard_flags = parsed.standard_flags.clone();
        resolved.command_matches = self.parse_command_matches(&parsed.remaining_args)?;

        for (key, value) in &self.defaults {
            resolved.apply_leaf(
                key,
                value.clone(),
                SourceInfo {
                    kind: SourceKind::Default,
                    location: format!("default[{key}]"),
                    precedence_level: 1,
                    raw: Some(render_value(value)),
                },
            );
        }

        self.load_discovered_level(
            &mut resolved,
            SourceKind::SystemFile,
            2,
            options
                .system_config_dir
                .join(format!("{}.json", self.app_name)),
            options
                .system_config_dir
                .join(format!("{}.toml", self.app_name)),
        )?;

        let binary_dir = self
            .executable_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        let install_dir = normalize_path(&binary_dir.join("../etc"));
        self.load_discovered_level(
            &mut resolved,
            SourceKind::InstallFile,
            3,
            install_dir.join(format!("{}.json", self.app_name)),
            install_dir.join(format!("{}.toml", self.app_name)),
        )?;

        if let Some(home_dir) = &options.home_dir {
            let config_dir = home_dir.join(".config").join(&self.app_name);
            self.load_discovered_level(
                &mut resolved,
                SourceKind::UserFile,
                4,
                config_dir.join("config.json"),
                config_dir.join("config.toml"),
            )?;
        }

        self.load_discovered_level(
            &mut resolved,
            SourceKind::WorkingDirFile,
            5,
            options.cwd.join(format!("{}.json", self.app_name)),
            options.cwd.join(format!("{}.toml", self.app_name)),
        )?;

        for (key, spec) in &self.schema {
            let env_name = self.environment_variable_name(key)?;
            if let Some(raw) = options.env.get(&env_name) {
                let value = parse_raw_for_value_type(key, spec.value_type(), raw)?;
                resolved.apply_leaf(
                    key,
                    value,
                    SourceInfo {
                        kind: SourceKind::Environment,
                        location: env_name,
                        precedence_level: 6,
                        raw: Some(raw.clone()),
                    },
                );
            }
        }

        for item in &parsed.config_files {
            let path = if item.path.is_absolute() {
                item.path.clone()
            } else {
                normalize_path(&options.cwd.join(&item.path))
            };
            self.load_config_file(&mut resolved, SourceKind::CliConfigFile, 7, path, true)?;
        }

        for item in &parsed.sets {
            validate_canonical_key(&item.key)?;
            let value = if let Some(spec) = self.schema.get(&item.key) {
                parse_raw_for_value_type(&item.key, spec.value_type(), &item.raw)?
            } else {
                parse_untyped_cli_value(&item.raw)
            };
            let value = self.normalize_declared_value(&item.key, value)?;

            resolved.apply_leaf(
                &item.key,
                value,
                SourceInfo {
                    kind: SourceKind::CliSet,
                    location: format!("argument {} of argv", item.argv_position),
                    precedence_level: 8,
                    raw: Some(item.raw.clone()),
                },
            );
        }

        let mut named_ops = Vec::new();
        for event in &parsed.explicit_scalar_flags {
            named_ops.push(NamedApply::Scalar(event.clone()));
        }
        for event in parsed.explicit_repeated_flags.values() {
            named_ops.push(NamedApply::Repeated(event.clone()));
        }
        named_ops.sort_by_key(|event| event.last_position());

        for named_op in named_ops {
            match named_op {
                NamedApply::Scalar(event) => {
                    let spec = self
                        .schema
                        .get(&event.key)
                        .expect("explicit scalar flags reference declared schema");
                    let value =
                        parse_raw_for_value_type(&event.key, spec.value_type(), &event.raw)?;
                    resolved.apply_leaf(
                        &event.key,
                        value,
                        SourceInfo {
                            kind: SourceKind::CliExplicit,
                            location: format!("argument {} of argv", event.argv_position),
                            precedence_level: 9,
                            raw: Some(event.raw),
                        },
                    );
                }
                NamedApply::Repeated(event) => {
                    let spec = self
                        .schema
                        .get(&event.key)
                        .expect("explicit repeated flags reference declared schema");
                    let element_type = spec
                        .value_type()
                        .repeated_element_type()
                        .expect("repeated flags always carry array value types");
                    let mut values = Vec::new();
                    let mut raw_parts = Vec::new();
                    let mut locations = Vec::new();
                    for item in &event.items {
                        values.push(parse_raw_for_value_type(
                            &event.key,
                            element_type,
                            &item.raw,
                        )?);
                        raw_parts.push(item.raw.clone());
                        locations.push(item.argv_position.to_string());
                    }
                    resolved.apply_leaf(
                        &event.key,
                        Value::Array(values),
                        SourceInfo {
                            kind: SourceKind::CliExplicit,
                            location: format!("arguments {} of argv", locations.join(", ")),
                            precedence_level: 9,
                            raw: Some(raw_parts.join(", ")),
                        },
                    );
                }
            }
        }

        Ok(resolved)
    }

    fn command_for_path(&self, path: &[String]) -> Result<&CommandSpec, LccpError> {
        let mut current = &self.root_command;
        let mut iter = path.iter();

        if let Some(first) = iter.next() {
            if first != current.name() {
                current = current
                    .subcommand(first)
                    .ok_or_else(|| LccpError::UnknownCommandPath(path.join(" ")))?;
            }
        }

        for segment in iter {
            current = current
                .subcommand(segment)
                .ok_or_else(|| LccpError::UnknownCommandPath(path.join(" ")))?;
        }

        Ok(current)
    }

    fn parse_command_matches(
        &self,
        remaining_args: &[(usize, String)],
    ) -> Result<CommandMatches, LccpError> {
        let mut index = 0;
        self.parse_command(&self.root_command, remaining_args, &mut index)
    }

    fn parse_command(
        &self,
        command: &CommandSpec,
        items: &[(usize, String)],
        index: &mut usize,
    ) -> Result<CommandMatches, LccpError> {
        let mut matches = CommandMatches::new(command.name().to_string());
        let mut option_occurrences: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut positional_occurrences: BTreeMap<String, Vec<Value>> = BTreeMap::new();
        let mut positional_index = 0usize;
        let mut stop_parsing_options = false;

        while *index < items.len() {
            let current = &items[*index].1;

            if !stop_parsing_options && current == "--" {
                stop_parsing_options = true;
                *index += 1;
                continue;
            }

            if !stop_parsing_options && current == "--help" {
                matches.help_requested = true;
                *index += 1;
                continue;
            }

            if !stop_parsing_options && current.starts_with("--") {
                let stripped = current
                    .strip_prefix("--")
                    .expect("checked starts_with(\"--\") above");
                let (flag, inline_value) = split_flag_value(stripped);
                let Some(option_name) = command.flag_index.get(flag).cloned() else {
                    return Err(LccpError::UnknownFlag(format!("--{flag}")));
                };
                let spec = command
                    .options
                    .get(&option_name)
                    .expect("command flag index only references declared options");

                let next_item = items.get(*index + 1).map(|(_, next)| next.as_str());
                let (value, consumed_next) = if let Some(inline_value) = inline_value {
                    (inline_value.to_string(), false)
                } else if matches!(spec.value_type(), ValueType::Boolean)
                    && next_item.map(should_use_implicit_bool).unwrap_or(true)
                {
                    ("true".to_string(), false)
                } else if spec.repeated_values()
                    && matches!(spec.value_type(), ValueType::Array(inner) if matches!(inner.as_ref(), ValueType::Boolean))
                    && next_item.map(should_use_implicit_bool).unwrap_or(true)
                {
                    ("true".to_string(), false)
                } else {
                    let (next_value, consumed_next) =
                        inline_or_next_value(None, items, *index, &format!("--{flag}"))?;
                    (next_value, consumed_next)
                };

                option_occurrences
                    .entry(option_name)
                    .or_default()
                    .push(value);
                *index += if consumed_next { 2 } else { 1 };
                continue;
            }

            if let Some(subcommand) = command.subcommand(current) {
                *index += 1;
                matches.subcommand = Some(Box::new(self.parse_command(subcommand, items, index)?));
                break;
            }

            let Some(spec) = command.positionals.get(positional_index) else {
                return Err(LccpError::UnexpectedPositional {
                    command: command.name().to_string(),
                    value: current.clone(),
                });
            };

            let value = parse_raw_for_value_type(spec.name(), spec.value_type(), current)?;
            positional_occurrences
                .entry(spec.name().to_string())
                .or_default()
                .push(value);

            if !spec.multiple_values() {
                positional_index += 1;
            }

            *index += 1;
        }

        for spec in command.options.values() {
            match option_occurrences.remove(spec.name()) {
                Some(values) if spec.repeated_values() => {
                    let element_type = spec
                        .value_type()
                        .repeated_element_type()
                        .expect("repeated command options always use array value types");
                    let parsed_values = values
                        .iter()
                        .map(|raw| parse_raw_for_value_type(spec.name(), element_type, raw))
                        .collect::<Result<Vec<_>, _>>()?;
                    matches
                        .options
                        .insert(spec.name().to_string(), Value::Array(parsed_values));
                }
                Some(values) => {
                    let raw = values
                        .last()
                        .expect("option occurrence vectors are never empty");
                    let value = parse_raw_for_value_type(spec.name(), spec.value_type(), raw)?;
                    matches.options.insert(spec.name().to_string(), value);
                }
                None if spec.required_option() => {
                    return Err(LccpError::MissingRequiredCliOption {
                        command: command.name().to_string(),
                        option: spec.name().to_string(),
                    });
                }
                None => {}
            }
        }

        for spec in command.positionals() {
            match positional_occurrences.remove(spec.name()) {
                Some(values) if spec.multiple_values() => {
                    matches
                        .positionals
                        .insert(spec.name().to_string(), Value::Array(values));
                }
                Some(values) => {
                    let value = values
                        .into_iter()
                        .next()
                        .expect("positional occurrence vectors are never empty");
                    matches.positionals.insert(spec.name().to_string(), value);
                }
                None if spec.required() => {
                    return Err(LccpError::MissingRequiredPositional {
                        command: command.name().to_string(),
                        positional: spec.name().to_string(),
                    });
                }
                None => {}
            }
        }

        Ok(matches)
    }

    fn normalize_declared_value(&self, key: &str, value: Value) -> Result<Value, LccpError> {
        if let Some(spec) = self.schema.get(key) {
            validate_value_matches_type(key, &value, spec.value_type())?;
        }
        Ok(value)
    }

    fn load_discovered_level(
        &self,
        resolved: &mut ResolvedConfig,
        source_kind: SourceKind,
        precedence_level: u8,
        json_path: PathBuf,
        toml_path: PathBuf,
    ) -> Result<(), LccpError> {
        let json_exists = json_path.is_file();
        let toml_exists = toml_path.is_file();

        if json_exists && toml_exists {
            resolved.insert_warning(format!(
                "Both discovered configuration files exist at level {precedence_level}: {} and {}. TOML overrides JSON at this level.",
                json_path.display(),
                toml_path.display()
            ));
        }

        if json_exists {
            self.load_config_file(resolved, source_kind, precedence_level, json_path, false)?;
        }

        if toml_exists {
            self.load_config_file(resolved, source_kind, precedence_level, toml_path, false)?;
        }

        Ok(())
    }

    fn load_config_file(
        &self,
        resolved: &mut ResolvedConfig,
        source_kind: SourceKind,
        precedence_level: u8,
        path: PathBuf,
        explicit_cli_file: bool,
    ) -> Result<(), LccpError> {
        let path_string = path.display().to_string();
        let content = fs::read_to_string(&path).map_err(|source| LccpError::ReadConfigFile {
            path: path_string.clone(),
            source,
            read_context: if explicit_cli_file {
                " (explicitly named by --config)".to_string()
            } else {
                String::new()
            },
        })?;

        let parsed = match config_format(&path)? {
            ConfigFormat::Json => serde_json::from_str::<Value>(&content).map_err(|error| {
                LccpError::ParseJsonConfig {
                    path: path_string.clone(),
                    message: error.to_string(),
                }
            })?,
            ConfigFormat::Toml => {
                let parsed =
                    content
                        .parse::<toml::Value>()
                        .map_err(|error| LccpError::ParseTomlConfig {
                            path: path_string.clone(),
                            message: error.to_string(),
                        })?;
                toml_to_json(&parsed)?
            }
        };

        let Value::Object(_) = parsed else {
            return Err(LccpError::InvalidConfigRoot(path_string));
        };

        let mut flattened = Vec::new();
        flatten_object("", &parsed, &mut flattened)?;

        for (key, value) in flattened {
            let value = self.normalize_declared_value(&key, value)?;
            resolved.apply_leaf(
                &key,
                value.clone(),
                SourceInfo {
                    kind: source_kind,
                    location: path.display().to_string(),
                    precedence_level,
                    raw: Some(render_value(&value)),
                },
            );
        }

        Ok(())
    }

    fn parse_argv<I, S>(&self, argv: I) -> Result<ParsedCli, LccpError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let raw_args: Vec<String> = argv
            .into_iter()
            .map(|item| item.as_ref().to_string())
            .collect();
        let skip_argv0 = raw_args
            .first()
            .map(|item| !item.starts_with('-'))
            .unwrap_or(false);

        let items: Vec<(usize, String)> = raw_args
            .into_iter()
            .enumerate()
            .filter(|(index, _)| !(skip_argv0 && *index == 0))
            .collect();

        let mut parsed = ParsedCli::default();
        let mut repeated_flags: BTreeMap<String, RepeatedNamedFlagEvent> = BTreeMap::new();

        let mut index = 0;
        while index < items.len() {
            let (original_index, current) = (&items[index].0, &items[index].1);
            if !current.starts_with("--") {
                parsed.remaining_args.push(items[index].clone());
                index += 1;
                continue;
            }

            let Some(stripped) = current.strip_prefix("--") else {
                index += 1;
                continue;
            };

            let (flag, inline_value) = split_flag_value(stripped);
            let argv_position = original_index + 1;

            match flag {
                "config" => {
                    let (value, consumed_next) =
                        inline_or_next_value(inline_value, &items, index, "--config")?;
                    parsed.config_files.push(ConfigFileArg {
                        path: PathBuf::from(value),
                    });
                    index += if consumed_next { 2 } else { 1 };
                }
                "set" => {
                    let (value, consumed_next) =
                        inline_or_next_value(inline_value, &items, index, "--set")?;
                    let Some((key, raw)) = value.split_once('=') else {
                        return Err(LccpError::InvalidSetSyntax(value));
                    };
                    parsed.sets.push(SetArg {
                        key: key.to_string(),
                        raw: raw.to_string(),
                        argv_position,
                    });
                    index += if consumed_next { 2 } else { 1 };
                }
                "print-default-config" => {
                    parsed.standard_flags.print_default_config = true;
                    index += 1;
                }
                "print-config" => {
                    parsed.standard_flags.print_config = true;
                    index += 1;
                }
                "debug-config" => {
                    parsed.standard_flags.debug_config = true;
                    index += 1;
                }
                _ => {
                    let Some(key) = self.flag_index.get(flag).cloned() else {
                        parsed.remaining_args.push(items[index].clone());
                        index += 1;
                        continue;
                    };
                    let spec = self
                        .schema
                        .get(&key)
                        .expect("flag index only references declared schema");
                    let next_item = items.get(index + 1).map(|(_, next)| next.as_str());
                    let (value, consumed_next) = if let Some(inline_value) = inline_value {
                        (inline_value.to_string(), false)
                    } else if matches!(spec.value_type(), ValueType::Boolean)
                        && next_item.map(should_use_implicit_bool).unwrap_or(true)
                    {
                        ("true".to_string(), false)
                    } else if spec.repeated_cli()
                        && matches!(spec.value_type(), ValueType::Array(inner) if matches!(inner.as_ref(), ValueType::Boolean))
                        && next_item.map(should_use_implicit_bool).unwrap_or(true)
                    {
                        ("true".to_string(), false)
                    } else {
                        let (next_value, consumed_next) =
                            inline_or_next_value(None, &items, index, &format!("--{flag}"))?;
                        (next_value, consumed_next)
                    };

                    if spec.repeated_cli() {
                        repeated_flags
                            .entry(key.clone())
                            .and_modify(|event| {
                                event.last_position = argv_position;
                                event.items.push(RepeatedNamedValue {
                                    raw: value.clone(),
                                    argv_position,
                                });
                            })
                            .or_insert_with(|| RepeatedNamedFlagEvent {
                                key: key.clone(),
                                last_position: argv_position,
                                items: vec![RepeatedNamedValue {
                                    raw: value,
                                    argv_position,
                                }],
                            });
                    } else {
                        parsed.explicit_scalar_flags.push(ScalarNamedFlagEvent {
                            key,
                            raw: value,
                            argv_position,
                        });
                    }

                    index += if consumed_next { 2 } else { 1 };
                }
            }
        }

        parsed.explicit_repeated_flags = repeated_flags;
        Ok(parsed)
    }
}

#[derive(Debug, Error)]
pub enum LccpError {
    #[error("invalid app name `{0}`")]
    InvalidAppName(String),
    #[error("invalid canonical key `{0}`")]
    InvalidCanonicalKey(String),
    #[error("invalid command name `{0}`")]
    InvalidCommandName(String),
    #[error("invalid argument name `{0}`")]
    InvalidArgumentName(String),
    #[error("invalid long flag name `--{0}`")]
    InvalidLongFlag(String),
    #[error("duplicate option declaration for `{0}`")]
    DuplicateOption(String),
    #[error("duplicate CLI option declaration for `{0}`")]
    DuplicateCliOption(String),
    #[error("duplicate explicit flag `--{0}`")]
    DuplicateFlag(String),
    #[error("duplicate positional declaration for `{0}`")]
    DuplicatePositional(String),
    #[error("invalid positional layout for command `{0}`")]
    InvalidPositionalLayout(String),
    #[error("duplicate subcommand declaration for `{0}`")]
    DuplicateSubcommand(String),
    #[error("missing value after `{0}`")]
    MissingArgumentValue(String),
    #[error("unknown explicit flag `{0}`")]
    UnknownFlag(String),
    #[error("unknown command path `{0}`")]
    UnknownCommandPath(String),
    #[error("missing required option `{option}` for command `{command}`")]
    MissingRequiredCliOption { command: String, option: String },
    #[error("missing required positional `{positional}` for command `{command}`")]
    MissingRequiredPositional { command: String, positional: String },
    #[error("unexpected positional `{value}` for command `{command}`")]
    UnexpectedPositional { command: String, value: String },
    #[error("invalid --set syntax `{0}`")]
    InvalidSetSyntax(String),
    #[error("unsupported configuration file format for `{0}`")]
    UnsupportedConfigFormat(String),
    #[error("failed to read configuration file `{path}`{read_context}: {source}")]
    ReadConfigFile {
        path: String,
        #[source]
        source: std::io::Error,
        read_context: String,
    },
    #[error("failed to parse JSON configuration `{path}`: {message}")]
    ParseJsonConfig { path: String, message: String },
    #[error("failed to parse TOML configuration `{path}`: {message}")]
    ParseTomlConfig { path: String, message: String },
    #[error("configuration file `{0}` must contain a root object/table")]
    InvalidConfigRoot(String),
    #[error("type conversion failed for `{key}`: expected {expected}, got `{raw}`")]
    TypeConversion {
        key: String,
        expected: String,
        raw: String,
    },
    #[error("type mismatch for `{key}`: expected {expected}, got {actual}")]
    TypeMismatch {
        key: String,
        expected: String,
        actual: String,
    },
    #[error("failed to serialize configuration as TOML: {0}")]
    SerializeToml(String),
}

#[derive(Debug, Clone, Default)]
struct ParsedCli {
    standard_flags: StandardFlags,
    config_files: Vec<ConfigFileArg>,
    sets: Vec<SetArg>,
    explicit_scalar_flags: Vec<ScalarNamedFlagEvent>,
    explicit_repeated_flags: BTreeMap<String, RepeatedNamedFlagEvent>,
    remaining_args: Vec<(usize, String)>,
}

#[derive(Debug, Clone)]
struct ConfigFileArg {
    path: PathBuf,
}

#[derive(Debug, Clone)]
struct SetArg {
    key: String,
    raw: String,
    argv_position: usize,
}

#[derive(Debug, Clone)]
struct ScalarNamedFlagEvent {
    key: String,
    raw: String,
    argv_position: usize,
}

#[derive(Debug, Clone)]
struct RepeatedNamedValue {
    raw: String,
    argv_position: usize,
}

#[derive(Debug, Clone)]
struct RepeatedNamedFlagEvent {
    key: String,
    last_position: usize,
    items: Vec<RepeatedNamedValue>,
}

#[derive(Debug, Clone)]
enum NamedApply {
    Scalar(ScalarNamedFlagEvent),
    Repeated(RepeatedNamedFlagEvent),
}

impl NamedApply {
    fn last_position(&self) -> usize {
        match self {
            Self::Scalar(event) => event.argv_position,
            Self::Repeated(event) => event.last_position,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum ConfigFormat {
    Json,
    Toml,
}

fn validate_app_name(app_name: &str) -> Result<(), LccpError> {
    if app_name.is_empty()
        || !app_name
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
    {
        return Err(LccpError::InvalidAppName(app_name.to_string()));
    }

    Ok(())
}

fn validate_command_name(name: &str) -> Result<(), LccpError> {
    if name.is_empty()
        || !name
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
    {
        return Err(LccpError::InvalidCommandName(name.to_string()));
    }

    Ok(())
}

fn validate_argument_name(name: &str) -> Result<(), LccpError> {
    if name.is_empty()
        || !name
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_')
    {
        return Err(LccpError::InvalidArgumentName(name.to_string()));
    }

    Ok(())
}

fn validate_long_flag(flag: &str) -> Result<(), LccpError> {
    if flag.is_empty()
        || flag.starts_with('-')
        || flag.ends_with('-')
        || !flag
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
    {
        return Err(LccpError::InvalidLongFlag(flag.to_string()));
    }

    Ok(())
}

fn validate_canonical_key(key: &str) -> Result<(), LccpError> {
    if key.is_empty() {
        return Err(LccpError::InvalidCanonicalKey(key.to_string()));
    }

    for segment in key.split('.') {
        if segment.is_empty()
            || !segment
                .chars()
                .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_')
        {
            return Err(LccpError::InvalidCanonicalKey(key.to_string()));
        }
    }

    Ok(())
}

fn canonical_key_to_flag(key: &str) -> String {
    key.replace('.', "-").replace('_', "-")
}

fn render_help_text(
    app_name: &str,
    command: &CommandSpec,
    path: &[String],
    config_options: Vec<&OptionSpec>,
) -> String {
    let mut out = String::new();
    let command_path = if path.is_empty() {
        app_name.to_string()
    } else if path.first().map(String::as_str) == Some(app_name) {
        path.join(" ")
    } else {
        format!("{app_name} {}", path.join(" "))
    };

    out.push_str(&format!("Usage: {command_path}"));
    if !config_options.is_empty() || !command.options().is_empty() {
        out.push_str(" [OPTIONS]");
    }
    if !command.subcommands().is_empty() {
        out.push_str(" [SUBCOMMAND]");
    }
    for positional in command.positionals() {
        if positional.required() {
            out.push_str(&format!(" <{}>", positional.name()));
        } else {
            out.push_str(&format!(" [{}]", positional.name()));
        }
        if positional.multiple_values() {
            out.push_str("...");
        }
    }
    out.push('\n');

    if let Some(help) = command.help() {
        out.push('\n');
        out.push_str(help);
        out.push('\n');
    }

    out.push_str("\nStandard Options:\n");
    out.push_str("  --help                   Show help for this command\n");
    out.push_str("  --config <PATH>          Load an additional JSON or TOML config file\n");
    out.push_str("  --set <KEY=VALUE>        Apply a generic configuration override\n");
    out.push_str("  --print-default-config   Emit default configuration as TOML\n");
    out.push_str("  --print-config           Emit resolved configuration as TOML\n");
    out.push_str("  --debug-config           Emit resolved configuration with provenance\n");

    if !config_options.is_empty() {
        out.push_str("\nConfiguration Options:\n");
        for spec in config_options {
            let value_name = spec
                .value_name()
                .map(str::to_string)
                .unwrap_or_else(|| default_value_name(spec.value_type()));
            let suffix = if matches!(spec.value_type(), ValueType::Boolean) {
                String::new()
            } else {
                format!(" <{value_name}>")
            };
            out.push_str(&format!("  --{}{}", spec.long_flag(), suffix));
            if let Some(help) = spec.help() {
                out.push_str(&pad_help_column(
                    help,
                    24 + suffix.len() + spec.long_flag().len(),
                ));
            }
            out.push('\n');
        }
    }

    if !command.options().is_empty() {
        out.push_str("\nOptions:\n");
        for spec in command.options().values() {
            let value_name = spec
                .value_name()
                .map(str::to_string)
                .unwrap_or_else(|| default_value_name(spec.value_type()));
            let suffix = if matches!(spec.value_type(), ValueType::Boolean) {
                String::new()
            } else {
                format!(" <{value_name}>")
            };
            out.push_str(&format!("  --{}{}", spec.long_flag(), suffix));
            if let Some(help) = spec.help() {
                out.push_str(&pad_help_column(
                    help,
                    24 + suffix.len() + spec.long_flag().len(),
                ));
            }
            out.push('\n');
        }
    }

    if !command.positionals().is_empty() {
        out.push_str("\nPositionals:\n");
        for spec in command.positionals() {
            let mut name = spec.name().to_string();
            if spec.multiple_values() {
                name.push_str("...");
            }
            out.push_str(&format!("  {name}"));
            if let Some(help) = spec.help() {
                out.push_str(&pad_help_column(help, 2 + name.len()));
            }
            out.push('\n');
        }
    }

    if !command.subcommands().is_empty() {
        out.push_str("\nSubcommands:\n");
        for subcommand in command.subcommands().values() {
            out.push_str(&format!("  {}", subcommand.name()));
            if let Some(help) = subcommand.help() {
                out.push_str(&pad_help_column(help, 2 + subcommand.name().len()));
            }
            out.push('\n');
        }
    }

    out
}

fn default_value_name(value_type: &ValueType) -> String {
    match value_type {
        ValueType::String => "STRING".to_string(),
        ValueType::Integer => "INTEGER".to_string(),
        ValueType::Float => "FLOAT".to_string(),
        ValueType::Boolean => "BOOL".to_string(),
        ValueType::Array(inner) => default_value_name(inner),
    }
}

fn pad_help_column(help: &str, current_width: usize) -> String {
    let padding = if current_width >= 24 {
        "  ".to_string()
    } else {
        " ".repeat(24 - current_width)
    };
    format!("{padding}{help}")
}

fn split_flag_value(flag: &str) -> (&str, Option<&str>) {
    match flag.split_once('=') {
        Some((name, value)) => (name, Some(value)),
        None => (flag, None),
    }
}

fn should_use_implicit_bool(next: &str) -> bool {
    next.starts_with("--")
        || !matches!(next, "true" | "TRUE" | "True" | "false" | "FALSE" | "False")
}

fn inline_or_next_value(
    inline_value: Option<&str>,
    items: &[(usize, String)],
    index: usize,
    flag_name: &str,
) -> Result<(String, bool), LccpError> {
    if let Some(value) = inline_value {
        return Ok((value.to_string(), false));
    }

    let Some((_, next)) = items.get(index + 1) else {
        return Err(LccpError::MissingArgumentValue(flag_name.to_string()));
    };

    if next.starts_with("--") {
        return Err(LccpError::MissingArgumentValue(flag_name.to_string()));
    }

    Ok((next.clone(), true))
}

fn config_format(path: &Path) -> Result<ConfigFormat, LccpError> {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("json") => Ok(ConfigFormat::Json),
        Some("toml") => Ok(ConfigFormat::Toml),
        _ => Err(LccpError::UnsupportedConfigFormat(
            path.display().to_string(),
        )),
    }
}

fn flatten_object(
    prefix: &str,
    value: &Value,
    out: &mut Vec<(String, Value)>,
) -> Result<(), LccpError> {
    match value {
        Value::Object(map) => {
            for (key, nested) in map {
                validate_canonical_key_segment(key)?;
                let next_prefix = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                flatten_object(&next_prefix, nested, out)?;
            }
            Ok(())
        }
        _ => {
            validate_canonical_key(prefix)?;
            out.push((prefix.to_string(), value.clone()));
            Ok(())
        }
    }
}

fn validate_canonical_key_segment(segment: &str) -> Result<(), LccpError> {
    if segment.is_empty()
        || !segment
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_')
    {
        return Err(LccpError::InvalidCanonicalKey(segment.to_string()));
    }

    Ok(())
}

fn remove_descendants(entries: &mut BTreeMap<String, ResolvedEntry>, key: &str) {
    let prefix = format!("{key}.");
    let descendants: Vec<String> = entries
        .keys()
        .filter(|candidate| candidate.starts_with(&prefix))
        .cloned()
        .collect();

    for descendant in descendants {
        entries.remove(&descendant);
    }
}

fn remove_ancestors(entries: &mut BTreeMap<String, ResolvedEntry>, key: &str) {
    let parts: Vec<&str> = key.split('.').collect();
    for len in 1..parts.len() {
        let ancestor = parts[..len].join(".");
        entries.remove(&ancestor);
    }
}

fn parse_raw_for_value_type(
    key: &str,
    value_type: &ValueType,
    raw: &str,
) -> Result<Value, LccpError> {
    match value_type {
        ValueType::String => Ok(Value::String(raw.to_string())),
        ValueType::Integer => raw
            .parse::<i64>()
            .map(|parsed| Value::Number(Number::from(parsed)))
            .map_err(|_| LccpError::TypeConversion {
                key: key.to_string(),
                expected: value_type.description(),
                raw: raw.to_string(),
            }),
        ValueType::Float => {
            let parsed = raw.parse::<f64>().map_err(|_| LccpError::TypeConversion {
                key: key.to_string(),
                expected: value_type.description(),
                raw: raw.to_string(),
            })?;
            let Some(number) = Number::from_f64(parsed) else {
                return Err(LccpError::TypeConversion {
                    key: key.to_string(),
                    expected: value_type.description(),
                    raw: raw.to_string(),
                });
            };
            Ok(Value::Number(number))
        }
        ValueType::Boolean => match raw {
            "true" | "TRUE" | "True" => Ok(Value::Bool(true)),
            "false" | "FALSE" | "False" => Ok(Value::Bool(false)),
            _ => Err(LccpError::TypeConversion {
                key: key.to_string(),
                expected: value_type.description(),
                raw: raw.to_string(),
            }),
        },
        ValueType::Array(element_type) => {
            let parsed = parse_untyped_cli_value(raw);
            match parsed {
                Value::Array(values) => {
                    let mut normalized = Vec::with_capacity(values.len());
                    for value in values {
                        validate_value_matches_type(key, &value, element_type)?;
                        normalized.push(value);
                    }
                    Ok(Value::Array(normalized))
                }
                _ => Err(LccpError::TypeConversion {
                    key: key.to_string(),
                    expected: value_type.description(),
                    raw: raw.to_string(),
                }),
            }
        }
    }
}

fn parse_untyped_cli_value(raw: &str) -> Value {
    if let Some(value) = parse_toml_expression(raw) {
        return value;
    }

    match raw {
        "true" | "TRUE" | "True" => Value::Bool(true),
        "false" | "FALSE" | "False" => Value::Bool(false),
        "null" => Value::Null,
        _ => {
            if let Ok(parsed) = raw.parse::<i64>() {
                return Value::Number(Number::from(parsed));
            }

            if raw.contains(['.', 'e', 'E']) {
                if let Ok(parsed) = raw.parse::<f64>() {
                    if let Some(number) = Number::from_f64(parsed) {
                        return Value::Number(number);
                    }
                }
            }

            Value::String(raw.to_string())
        }
    }
}

fn parse_toml_expression(raw: &str) -> Option<Value> {
    let wrapped = format!("value = {raw}");
    let parsed = wrapped.parse::<toml::Table>().ok()?;
    let value = parsed.get("value")?;
    toml_to_json(value).ok()
}

fn validate_value_matches_type(
    key: &str,
    value: &Value,
    value_type: &ValueType,
) -> Result<(), LccpError> {
    match value_type {
        ValueType::String if matches!(value, Value::String(_)) => Ok(()),
        ValueType::Integer if is_integer(value) => Ok(()),
        ValueType::Float if is_float(value) || is_integer(value) => Ok(()),
        ValueType::Boolean if matches!(value, Value::Bool(_)) => Ok(()),
        ValueType::Array(element_type) => match value {
            Value::Array(values) => {
                for value in values {
                    validate_value_matches_type(key, value, element_type)?;
                }
                Ok(())
            }
            _ => Err(LccpError::TypeMismatch {
                key: key.to_string(),
                expected: value_type.description(),
                actual: value_kind(value).to_string(),
            }),
        },
        _ => Err(LccpError::TypeMismatch {
            key: key.to_string(),
            expected: value_type.description(),
            actual: value_kind(value).to_string(),
        }),
    }
}

fn is_integer(value: &Value) -> bool {
    matches!(value, Value::Number(number) if number.is_i64() || number.is_u64())
}

fn is_float(value: &Value) -> bool {
    matches!(value, Value::Number(number) if number.is_f64())
}

fn value_kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(number) if number.is_i64() || number.is_u64() => "integer",
        Value::Number(_) => "float",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn toml_to_json(value: &toml::Value) -> Result<Value, LccpError> {
    Ok(match value {
        toml::Value::String(value) => Value::String(value.clone()),
        toml::Value::Integer(value) => Value::Number(Number::from(*value)),
        toml::Value::Float(value) => {
            let Some(number) = Number::from_f64(*value) else {
                return Err(LccpError::SerializeToml(
                    "TOML float value is not finite".to_string(),
                ));
            };
            Value::Number(number)
        }
        toml::Value::Boolean(value) => Value::Bool(*value),
        toml::Value::Datetime(value) => Value::String(value.to_string()),
        toml::Value::Array(values) => Value::Array(
            values
                .iter()
                .map(toml_to_json)
                .collect::<Result<Vec<_>, _>>()?,
        ),
        toml::Value::Table(map) => {
            let mut out = JsonMap::new();
            for (key, value) in map {
                out.insert(key.clone(), toml_to_json(value)?);
            }
            Value::Object(out)
        }
    })
}

fn emit_flat_map_as_toml<'a, I>(entries: I) -> Result<String, LccpError>
where
    I: IntoIterator<Item = (&'a str, &'a Value)>,
{
    let mut root = Value::Object(JsonMap::new());
    for (key, value) in entries {
        insert_value_at_path(&mut root, key, value.clone());
    }

    let toml_value = json_to_toml(&root)?;
    toml::to_string_pretty(&toml_value).map_err(|error| LccpError::SerializeToml(error.to_string()))
}

fn json_to_toml(value: &Value) -> Result<toml::Value, LccpError> {
    Ok(match value {
        Value::Null => {
            return Err(LccpError::SerializeToml(
                "null values cannot be emitted as TOML".to_string(),
            ))
        }
        Value::Bool(value) => toml::Value::Boolean(*value),
        Value::Number(value) => {
            if let Some(value) = value.as_i64() {
                toml::Value::Integer(value)
            } else if let Some(value) = value.as_u64() {
                let converted = i64::try_from(value).map_err(|_| {
                    LccpError::SerializeToml(format!(
                        "integer value {value} is too large for TOML emission"
                    ))
                })?;
                toml::Value::Integer(converted)
            } else if let Some(value) = value.as_f64() {
                toml::Value::Float(value)
            } else {
                return Err(LccpError::SerializeToml(
                    "unsupported JSON number representation".to_string(),
                ));
            }
        }
        Value::String(value) => toml::Value::String(value.clone()),
        Value::Array(values) => toml::Value::Array(
            values
                .iter()
                .map(json_to_toml)
                .collect::<Result<Vec<_>, _>>()?,
        ),
        Value::Object(map) => {
            let mut table = toml::map::Map::new();
            for (key, value) in map {
                table.insert(key.clone(), json_to_toml(value)?);
            }
            toml::Value::Table(table)
        }
    })
}

fn insert_value_at_path(root: &mut Value, key: &str, value: Value) {
    let parts: Vec<&str> = key.split('.').collect();
    insert_path_parts(root, &parts, value);
}

fn insert_path_parts(root: &mut Value, parts: &[&str], value: Value) {
    if parts.is_empty() {
        *root = value;
        return;
    }

    if !root.is_object() {
        *root = Value::Object(JsonMap::new());
    }

    let object = root.as_object_mut().expect("root converted to object");
    if parts.len() == 1 {
        object.insert(parts[0].to_string(), value);
        return;
    }

    let entry = object
        .entry(parts[0].to_string())
        .or_insert_with(|| Value::Object(JsonMap::new()));
    insert_path_parts(entry, &parts[1..], value);
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();

    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }

    normalized
}

fn render_value(value: &Value) -> String {
    match value {
        Value::String(value) => format!("{value:?}"),
        _ => value.to_string(),
    }
}
