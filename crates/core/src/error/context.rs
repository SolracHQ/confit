//! Typed error context for log records.
//!
//! Context names where and with what one operation ran.

use std::path::PathBuf;

/// Document source as a fixed fact.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Source {
    /// The source kind.
    pub kind: SourceKind,
    /// The source name.
    pub name: String,
    /// The declaring location, when one names it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub declared_at: Option<String>,
}

impl Source {
    /// Build one document source.
    pub fn new(kind: SourceKind, name: impl Into<String>, declared_at: Option<String>) -> Self {
        Self {
            kind,
            name: name.into(),
            declared_at,
        }
    }
}

/// Document source kind as a fixed fact.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    /// Profile source.
    Profile,
    /// Config source.
    Config,
}

impl std::fmt::Display for SourceKind {
    /// Render one source kind.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Profile => write!(formatter, "profile"),
            Self::Config => write!(formatter, "config"),
        }
    }
}

/// Typed error context for log records.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct ErrorContext {
    /// Attempted filesystem path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fs_path: Option<PathBuf>,
    /// Canonical filesystem path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub canonical_path: Option<PathBuf>,
    /// Cache path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_path: Option<PathBuf>,
    /// Bundle path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bundle_path: Option<PathBuf>,
    /// Bundle member.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub member: Option<String>,
    /// Module name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module: Option<String>,
    /// Scope path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<PathBuf>,
    /// Profile path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<PathBuf>,
    /// Project path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<PathBuf>,
    /// Document route.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub document: Option<String>,
    /// Lookup id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Near-match candidates.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub suggestions: Vec<String>,
    /// Request url.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Require cycle.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub cycle: Vec<String>,
    /// Document sources.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<Source>,
    /// Hook argv.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub argv: Option<String>,
    /// Slot name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slot: Option<String>,
    /// Command name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// Conflicting flags.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub flags: Vec<String>,
    /// Backend detail.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// Stack trace lines.
    #[serde(skip_serializing)]
    pub trace: Vec<String>,
}

impl ErrorContext {
    /// Build one empty error context.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the attempted filesystem path.
    pub fn with_fs_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.fs_path = Some(path.into());
        self
    }

    /// Set the canonical filesystem path.
    pub fn with_canonical_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.canonical_path = Some(path.into());
        self
    }

    /// Set the cache path.
    pub fn with_cache_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.cache_path = Some(path.into());
        self
    }

    /// Set the bundle path.
    pub fn with_bundle_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.bundle_path = Some(path.into());
        self
    }

    /// Set the bundle member.
    pub fn with_member(mut self, member: impl Into<String>) -> Self {
        self.member = Some(member.into());
        self
    }

    /// Set the module name.
    pub fn with_module(mut self, name: impl Into<String>) -> Self {
        self.module = Some(name.into());
        self
    }

    /// Set the scope path.
    pub fn with_scope(mut self, path: impl Into<PathBuf>) -> Self {
        self.scope = Some(path.into());
        self
    }

    /// Set the profile path.
    pub fn with_profile(mut self, path: impl Into<PathBuf>) -> Self {
        self.profile = Some(path.into());
        self
    }

    /// Set the project path.
    pub fn with_project(mut self, path: impl Into<PathBuf>) -> Self {
        self.project = Some(path.into());
        self
    }

    /// Set the document route.
    pub fn with_document(mut self, route: impl Into<String>) -> Self {
        self.document = Some(route.into());
        self
    }

    /// Set the lookup id.
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Set the near-match candidates.
    pub fn with_suggestions(mut self, suggestions: Vec<String>) -> Self {
        self.suggestions = suggestions;
        self
    }

    /// Set the request url.
    pub fn with_url(mut self, url: impl Into<String>) -> Self {
        self.url = Some(url.into());
        self
    }

    /// Set the require cycle.
    pub fn with_cycle(mut self, chain: Vec<String>) -> Self {
        self.cycle = chain;
        self
    }

    /// Set the hook argv.
    pub fn with_argv(mut self, argv: impl Into<String>) -> Self {
        self.argv = Some(argv.into());
        self
    }

    /// Set the slot name.
    pub fn with_slot(mut self, slot: impl Into<String>) -> Self {
        self.slot = Some(slot.into());
        self
    }

    /// Set the command name.
    pub fn with_command(mut self, command: impl Into<String>) -> Self {
        self.command = Some(command.into());
        self
    }

    /// Set the conflicting flags.
    pub fn with_flags(mut self, flags: Vec<String>) -> Self {
        self.flags = flags;
        self
    }

    /// Set the backend detail.
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    /// Set the stack trace lines.
    pub fn with_trace(mut self, trace: Vec<String>) -> Self {
        self.trace = trace;
        self
    }

    /// Push one document source.
    pub fn push_source(mut self, source: Source) -> Self {
        self.sources.push(source);
        self
    }

    /// Report whether every slot stays unset.
    pub fn is_empty(&self) -> bool {
        self.fs_path.is_none()
            && self.canonical_path.is_none()
            && self.cache_path.is_none()
            && self.bundle_path.is_none()
            && self.member.is_none()
            && self.module.is_none()
            && self.scope.is_none()
            && self.profile.is_none()
            && self.project.is_none()
            && self.document.is_none()
            && self.id.is_none()
            && self.suggestions.is_empty()
            && self.url.is_none()
            && self.cycle.is_empty()
            && self.argv.is_none()
            && self.slot.is_none()
            && self.command.is_none()
            && self.flags.is_empty()
            && self.detail.is_none()
            && self.trace.is_empty()
            && self.sources.is_empty()
    }

    /// Render the set slots as one JSON object for the log record.
    pub fn log_value(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
    }
}

impl std::fmt::Display for ErrorContext {
    /// Render the log object for direct printing.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_empty() {
            return write!(formatter, "no context");
        }
        write!(formatter, "{}", self.log_value())
    }
}

impl std::error::Error for ErrorContext {}
