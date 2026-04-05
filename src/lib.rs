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
        })
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

#[derive(Debug, Clone, Default)]
pub struct ResolvedConfig {
    entries: BTreeMap<String, ResolvedEntry>,
    warnings: Vec<String>,
    standard_flags: StandardFlags,
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
}

impl Resolver {
    pub fn new(
        app_name: impl Into<String>,
        executable_path: impl Into<PathBuf>,
    ) -> Result<Self, LccpError> {
        let app_name = app_name.into();
        validate_app_name(&app_name)?;

        Ok(Self {
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
        let mut resolved = ResolvedConfig::default();
        resolved.standard_flags = parsed.standard_flags.clone();

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
                        return Err(LccpError::UnknownFlag(format!("--{flag}")));
                    };
                    let spec = self
                        .schema
                        .get(&key)
                        .expect("flag index only references declared schema");
                    let (value, consumed_next) = if let Some(inline_value) = inline_value {
                        (inline_value.to_string(), false)
                    } else if matches!(spec.value_type(), ValueType::Boolean)
                        && items
                            .get(index + 1)
                            .map(|(_, next)| next.starts_with("--"))
                            .unwrap_or(true)
                    {
                        ("true".to_string(), false)
                    } else if spec.repeated_cli()
                        && matches!(spec.value_type(), ValueType::Array(inner) if matches!(inner.as_ref(), ValueType::Boolean))
                        && items
                            .get(index + 1)
                            .map(|(_, next)| next.starts_with("--"))
                            .unwrap_or(true)
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
    #[error("duplicate option declaration for `{0}`")]
    DuplicateOption(String),
    #[error("duplicate explicit flag `--{0}`")]
    DuplicateFlag(String),
    #[error("missing value after `{0}`")]
    MissingArgumentValue(String),
    #[error("unknown explicit flag `{0}`")]
    UnknownFlag(String),
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

fn split_flag_value(flag: &str) -> (&str, Option<&str>) {
    match flag.split_once('=') {
        Some((name, value)) => (name, Some(value)),
        None => (flag, None),
    }
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
