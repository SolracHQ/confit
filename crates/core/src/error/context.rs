//! Typed context for log records.

use std::path::PathBuf;

/// A document source.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Source {
    /// The source kind.
    pub kind: SourceKind,
    /// The source name.
    pub name: String,
    /// The declaring location, when the source has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub declared_at: Option<String>,
}

impl Source {
    /// Builds a source for the log context.
    pub fn new(kind: SourceKind, name: impl Into<String>, declared_at: Option<String>) -> Self {
        Self {
            kind,
            name: name.into(),
            declared_at,
        }
    }
}

/// A document source kind.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    /// A source that comes from a profile.
    Profile,
    /// A source that comes from a config.
    Config,
}

/// The typed context of one failure.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct ErrorContext {
    /// The path that the operation attempted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fs_path: Option<PathBuf>,
    /// The path after canonical resolution.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub canonical_path: Option<PathBuf>,
    /// The path of the content cache.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_path: Option<PathBuf>,
    /// The path of the bundle.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bundle_path: Option<PathBuf>,
    /// The name of the bundle member.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub member: Option<String>,
    /// The name of the module under load.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module: Option<String>,
    /// The path of the active scope.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<PathBuf>,
    /// The path of the profile file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<PathBuf>,
    /// The path of the project folder.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<PathBuf>,
    /// The route of the open document.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub document: Option<String>,
    /// The id used for the lookup.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// The candidates near the wanted name.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub suggestions: Vec<String>,
    /// The url of the request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// The cycle in the require chain.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub cycle: Vec<String>,
    /// The sources of the document.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<Source>,
    /// The argv of the hook.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub argv: Option<String>,
    /// The name of the stored state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    /// The name of the command.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// The flags that conflict.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub flags: Vec<String>,
    /// The detail from the backend.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// The lines of the stack trace.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub trace: Vec<String>,
}

impl ErrorContext {
    /// Builds one empty context.
    pub fn new() -> Self {
        Self::default()
    }

    /// Records the path that the operation attempted.
    pub fn with_fs_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.fs_path = Some(path.into());
        self
    }

    /// Records the path after canonical resolution.
    pub fn with_canonical_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.canonical_path = Some(path.into());
        self
    }

    /// Records the path of the content cache.
    pub fn with_cache_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.cache_path = Some(path.into());
        self
    }

    /// Records the path of the bundle.
    pub fn with_bundle_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.bundle_path = Some(path.into());
        self
    }

    /// Records the name of the bundle member.
    pub fn with_member(mut self, member: impl Into<String>) -> Self {
        self.member = Some(member.into());
        self
    }

    /// Records the name of the module under load.
    pub fn with_module(mut self, name: impl Into<String>) -> Self {
        self.module = Some(name.into());
        self
    }

    /// Records the path of the active scope.
    pub fn with_scope(mut self, path: impl Into<PathBuf>) -> Self {
        self.scope = Some(path.into());
        self
    }

    /// Records the path of the profile file.
    pub fn with_profile(mut self, path: impl Into<PathBuf>) -> Self {
        self.profile = Some(path.into());
        self
    }

    /// Records the path of the project folder.
    pub fn with_project(mut self, path: impl Into<PathBuf>) -> Self {
        self.project = Some(path.into());
        self
    }

    /// Records the route of the open document.
    pub fn with_document(mut self, route: impl Into<String>) -> Self {
        self.document = Some(route.into());
        self
    }

    /// Records the id used for the lookup.
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Records the candidates near the wanted name.
    pub fn with_suggestions(mut self, suggestions: Vec<String>) -> Self {
        self.suggestions = suggestions;
        self
    }

    /// Records the url of the request.
    pub fn with_url(mut self, url: impl Into<String>) -> Self {
        self.url = Some(url.into());
        self
    }

    /// Records the cycle in the require chain.
    pub fn with_cycle(mut self, chain: Vec<String>) -> Self {
        self.cycle = chain;
        self
    }

    /// Records the argv of the hook.
    pub fn with_argv(mut self, argv: impl Into<String>) -> Self {
        self.argv = Some(argv.into());
        self
    }

    /// Records the name of the stored state.
    pub fn with_state(mut self, state: impl Into<String>) -> Self {
        self.state = Some(state.into());
        self
    }

    /// Records the name of the command.
    pub fn with_command(mut self, command: impl Into<String>) -> Self {
        self.command = Some(command.into());
        self
    }

    /// Records the flags that conflict.
    pub fn with_flags(mut self, flags: Vec<String>) -> Self {
        self.flags = flags;
        self
    }

    /// Records the detail from the backend.
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    /// Records the lines of the stack trace.
    pub fn with_trace(mut self, trace: Vec<String>) -> Self {
        self.trace = trace;
        self
    }

    /// Adds one document source to the context.
    pub fn push_source(mut self, source: Source) -> Self {
        self.sources.push(source);
        self
    }

    /// Returns whether every state is unset.
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
            && self.state.is_none()
            && self.command.is_none()
            && self.flags.is_empty()
            && self.detail.is_none()
            && self.trace.is_empty()
            && self.sources.is_empty()
    }

    /// Builds the log object from the states that are set.
    pub fn log_value(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
    }
}

impl std::fmt::Display for ErrorContext {
    /// Prints the context as its log object.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_empty() {
            return write!(formatter, "no context");
        }
        write!(formatter, "{}", self.log_value())
    }
}

impl std::error::Error for ErrorContext {}
