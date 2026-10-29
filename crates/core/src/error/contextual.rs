//! Contextual causes carrying context.
//!
//! One contextual cause joins the unexpected result with where it ran.
//! Terminal layers add the required operation on top.

use super::cause::{ErrorCause, EvalError};
use super::context::{ErrorContext, Source};

/// Shared contextual error result.
pub type ContextualResult<T> = std::result::Result<T, ContextualCause>;

/// Build one contextual cause.
pub fn contextual(cause: impl Into<ErrorCause>) -> ContextualCause {
    ContextualCause::new(cause)
}

/// Unexpected result with where it ran.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextualCause {
    /// Failed result.
    pub cause: ErrorCause,
    /// Where the operation ran.
    pub context: ErrorContext,
}

impl ContextualCause {
    /// Build one contextual cause.
    pub fn new(cause: impl Into<ErrorCause>) -> Self {
        Self {
            cause: cause.into(),
            context: ErrorContext::new(),
        }
    }

    /// Return the failed result.
    pub fn cause(&self) -> &ErrorCause {
        &self.cause
    }

    /// Return where the operation ran.
    pub fn context(&self) -> &ErrorContext {
        &self.context
    }

    /// Set the attempted filesystem path.
    pub fn with_fs_path(mut self, path: impl Into<std::path::PathBuf>) -> Self {
        self.context = std::mem::take(&mut self.context).with_fs_path(path);
        self
    }

    /// Set the canonical filesystem path.
    pub fn with_canonical_path(mut self, path: impl Into<std::path::PathBuf>) -> Self {
        self.context = std::mem::take(&mut self.context).with_canonical_path(path);
        self
    }

    /// Set the cache path.
    pub fn with_cache_path(mut self, path: impl Into<std::path::PathBuf>) -> Self {
        self.context = std::mem::take(&mut self.context).with_cache_path(path);
        self
    }

    /// Set the bundle path.
    pub fn with_bundle_path(mut self, path: impl Into<std::path::PathBuf>) -> Self {
        self.context = std::mem::take(&mut self.context).with_bundle_path(path);
        self
    }

    /// Set the bundle member.
    pub fn with_member(mut self, member: impl Into<String>) -> Self {
        self.context = std::mem::take(&mut self.context).with_member(member);
        self
    }

    /// Set the module name.
    pub fn with_module(mut self, name: impl Into<String>) -> Self {
        self.context = std::mem::take(&mut self.context).with_module(name);
        self
    }

    /// Set the scope path.
    pub fn with_scope(mut self, path: impl Into<std::path::PathBuf>) -> Self {
        self.context = std::mem::take(&mut self.context).with_scope(path);
        self
    }

    /// Set the profile path.
    pub fn with_profile(mut self, path: impl Into<std::path::PathBuf>) -> Self {
        self.context = std::mem::take(&mut self.context).with_profile(path);
        self
    }

    /// Set the project path.
    pub fn with_project(mut self, path: impl Into<std::path::PathBuf>) -> Self {
        self.context = std::mem::take(&mut self.context).with_project(path);
        self
    }

    /// Set the document route.
    pub fn with_document(mut self, route: impl Into<String>) -> Self {
        self.context = std::mem::take(&mut self.context).with_document(route);
        self
    }

    /// Set the lookup id.
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.context = std::mem::take(&mut self.context).with_id(id);
        self
    }

    /// Set the near-match candidates.
    pub fn with_suggestions(mut self, suggestions: Vec<String>) -> Self {
        self.context = std::mem::take(&mut self.context).with_suggestions(suggestions);
        self
    }

    /// Set the request url.
    pub fn with_url(mut self, url: impl Into<String>) -> Self {
        self.context = std::mem::take(&mut self.context).with_url(url);
        self
    }

    /// Set the require cycle.
    pub fn with_cycle(mut self, chain: Vec<String>) -> Self {
        self.context = std::mem::take(&mut self.context).with_cycle(chain);
        self
    }

    /// Set the hook argv.
    pub fn with_argv(mut self, argv: impl Into<String>) -> Self {
        self.context = std::mem::take(&mut self.context).with_argv(argv);
        self
    }

    /// Set the slot name.
    pub fn with_slot(mut self, slot: impl Into<String>) -> Self {
        self.context = std::mem::take(&mut self.context).with_slot(slot);
        self
    }

    /// Set the command name.
    pub fn with_command(mut self, command: impl Into<String>) -> Self {
        self.context = std::mem::take(&mut self.context).with_command(command);
        self
    }

    /// Set the conflicting flags.
    pub fn with_flags(mut self, flags: Vec<String>) -> Self {
        self.context = std::mem::take(&mut self.context).with_flags(flags);
        self
    }

    /// Set the backend detail.
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.context = std::mem::take(&mut self.context).with_detail(detail);
        self
    }

    /// Set the stack trace lines.
    pub fn with_trace(mut self, trace: Vec<String>) -> Self {
        self.context = std::mem::take(&mut self.context).with_trace(trace);
        self
    }

    /// Push one document source.
    pub fn push_source(mut self, source: Source) -> Self {
        self.context = std::mem::take(&mut self.context).push_source(source);
        self
    }

    /// Close one contextual cause into its terminal error.
    pub fn terminal(self, kind: super::kind::ErrorKind) -> super::terminal::Error {
        super::terminal::Error::new(kind, self)
    }
}

/// Attach context to one contextual result without mapping its error by hand.
pub trait WithContext<T> {
    /// Set the attempted filesystem path.
    fn with_fs_path(self, path: impl Into<std::path::PathBuf>) -> ContextualResult<T>;
    /// Set the canonical filesystem path.
    fn with_canonical_path(self, path: impl Into<std::path::PathBuf>) -> ContextualResult<T>;
    /// Set the cache path.
    fn with_cache_path(self, path: impl Into<std::path::PathBuf>) -> ContextualResult<T>;
    /// Set the bundle path.
    fn with_bundle_path(self, path: impl Into<std::path::PathBuf>) -> ContextualResult<T>;
    /// Set the bundle member.
    fn with_member(self, member: impl Into<String>) -> ContextualResult<T>;
    /// Set the module name.
    fn with_module(self, name: impl Into<String>) -> ContextualResult<T>;
    /// Set the scope path.
    fn with_scope(self, path: impl Into<std::path::PathBuf>) -> ContextualResult<T>;
    /// Set the profile path.
    fn with_profile(self, path: impl Into<std::path::PathBuf>) -> ContextualResult<T>;
    /// Set the project path.
    fn with_project(self, path: impl Into<std::path::PathBuf>) -> ContextualResult<T>;
    /// Set the document route.
    fn with_document(self, route: impl Into<String>) -> ContextualResult<T>;
    /// Set the lookup id.
    fn with_id(self, id: impl Into<String>) -> ContextualResult<T>;
    /// Set the near-match candidates.
    fn with_suggestions(self, suggestions: Vec<String>) -> ContextualResult<T>;
    /// Set the request url.
    fn with_url(self, url: impl Into<String>) -> ContextualResult<T>;
    /// Set the require cycle.
    fn with_cycle(self, chain: Vec<String>) -> ContextualResult<T>;
    /// Set the hook argv.
    fn with_argv(self, argv: impl Into<String>) -> ContextualResult<T>;
    /// Set the slot name.
    fn with_slot(self, slot: impl Into<String>) -> ContextualResult<T>;
    /// Set the command name.
    fn with_command(self, command: impl Into<String>) -> ContextualResult<T>;
    /// Set the conflicting flags.
    fn with_flags(self, flags: Vec<String>) -> ContextualResult<T>;
    /// Set the backend detail.
    fn with_detail(self, detail: impl Into<String>) -> ContextualResult<T>;
    /// Set the stack trace lines.
    fn with_trace(self, trace: Vec<String>) -> ContextualResult<T>;
    /// Push one document source.
    fn push_source(self, source: Source) -> ContextualResult<T>;
}

impl<T> WithContext<T> for ContextualResult<T> {
    /// Set the attempted filesystem path.
    fn with_fs_path(self, path: impl Into<std::path::PathBuf>) -> ContextualResult<T> {
        self.map_err(|contextual| contextual.with_fs_path(path))
    }
    /// Set the canonical filesystem path.
    fn with_canonical_path(self, path: impl Into<std::path::PathBuf>) -> ContextualResult<T> {
        self.map_err(|contextual| contextual.with_canonical_path(path))
    }
    /// Set the cache path.
    fn with_cache_path(self, path: impl Into<std::path::PathBuf>) -> ContextualResult<T> {
        self.map_err(|contextual| contextual.with_cache_path(path))
    }
    /// Set the bundle path.
    fn with_bundle_path(self, path: impl Into<std::path::PathBuf>) -> ContextualResult<T> {
        self.map_err(|contextual| contextual.with_bundle_path(path))
    }
    /// Set the bundle member.
    fn with_member(self, member: impl Into<String>) -> ContextualResult<T> {
        self.map_err(|contextual| contextual.with_member(member))
    }
    /// Set the module name.
    fn with_module(self, name: impl Into<String>) -> ContextualResult<T> {
        self.map_err(|contextual| contextual.with_module(name))
    }
    /// Set the scope path.
    fn with_scope(self, path: impl Into<std::path::PathBuf>) -> ContextualResult<T> {
        self.map_err(|contextual| contextual.with_scope(path))
    }
    /// Set the profile path.
    fn with_profile(self, path: impl Into<std::path::PathBuf>) -> ContextualResult<T> {
        self.map_err(|contextual| contextual.with_profile(path))
    }
    /// Set the project path.
    fn with_project(self, path: impl Into<std::path::PathBuf>) -> ContextualResult<T> {
        self.map_err(|contextual| contextual.with_project(path))
    }
    /// Set the document route.
    fn with_document(self, route: impl Into<String>) -> ContextualResult<T> {
        self.map_err(|contextual| contextual.with_document(route))
    }
    /// Set the lookup id.
    fn with_id(self, id: impl Into<String>) -> ContextualResult<T> {
        self.map_err(|contextual| contextual.with_id(id))
    }
    /// Set the near-match candidates.
    fn with_suggestions(self, suggestions: Vec<String>) -> ContextualResult<T> {
        self.map_err(|contextual| contextual.with_suggestions(suggestions))
    }
    /// Set the request url.
    fn with_url(self, url: impl Into<String>) -> ContextualResult<T> {
        self.map_err(|contextual| contextual.with_url(url))
    }
    /// Set the require cycle.
    fn with_cycle(self, chain: Vec<String>) -> ContextualResult<T> {
        self.map_err(|contextual| contextual.with_cycle(chain))
    }
    /// Set the hook argv.
    fn with_argv(self, argv: impl Into<String>) -> ContextualResult<T> {
        self.map_err(|contextual| contextual.with_argv(argv))
    }
    /// Set the slot name.
    fn with_slot(self, slot: impl Into<String>) -> ContextualResult<T> {
        self.map_err(|contextual| contextual.with_slot(slot))
    }
    /// Set the command name.
    fn with_command(self, command: impl Into<String>) -> ContextualResult<T> {
        self.map_err(|contextual| contextual.with_command(command))
    }
    /// Set the conflicting flags.
    fn with_flags(self, flags: Vec<String>) -> ContextualResult<T> {
        self.map_err(|contextual| contextual.with_flags(flags))
    }
    /// Set the backend detail.
    fn with_detail(self, detail: impl Into<String>) -> ContextualResult<T> {
        self.map_err(|contextual| contextual.with_detail(detail))
    }
    /// Set the stack trace lines.
    fn with_trace(self, trace: Vec<String>) -> ContextualResult<T> {
        self.map_err(|contextual| contextual.with_trace(trace))
    }
    /// Push one document source.
    fn push_source(self, source: Source) -> ContextualResult<T> {
        self.map_err(|contextual| contextual.push_source(source))
    }
}

impl std::fmt::Display for ContextualCause {
    /// Render one cause with its log object.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.context.is_empty() {
            return write!(formatter, "{}", self.cause);
        }
        write!(formatter, "{} {}", self.cause, self.context)
    }
}

impl std::error::Error for ContextualCause {
    /// Report one cause.
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.cause)
    }
}

impl<T> From<T> for ContextualCause
where
    T: Into<ErrorCause>,
{
    /// Wrap one cause-like value.
    fn from(cause: T) -> Self {
        Self::new(cause.into())
    }
}

impl From<mlua::Error> for ContextualCause {
    /// Convert one Lua failure.
    fn from(error: mlua::Error) -> Self {
        if let Some(known) = error.downcast_ref::<Self>().cloned() {
            return known;
        }
        let text = error.to_string();
        let mut lines = text.lines();
        let detail = lines.next().unwrap_or_default().to_owned();
        Self::new(EvalError::LuaFailed)
            .with_detail(detail)
            .with_trace(lines.map(str::to_owned).collect())
    }
}
