//! Failure causes that carry where the operation ran.

use super::cause::{ErrorCause, EvalError};
use super::context::{ErrorContext, Source};

/// One contextual result.
pub type ContextualResult<T> = std::result::Result<T, ContextualCause>;

/// Wraps one cause with its location.
pub fn contextual(cause: impl Into<ErrorCause>) -> ContextualCause {
    ContextualCause::new(cause)
}

/// A failure cause and its location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextualCause {
    /// The cause that failed.
    pub cause: ErrorCause,
    /// The location where the operation ran.
    pub context: ErrorContext,
}

impl ContextualCause {
    /// Creates one cause with an empty location.
    pub fn new(cause: impl Into<ErrorCause>) -> Self {
        Self {
            cause: cause.into(),
            context: ErrorContext::new(),
        }
    }

    /// Returns the cause that failed.
    pub fn cause(&self) -> &ErrorCause {
        &self.cause
    }

    /// Returns the location where the operation ran.
    pub fn context(&self) -> &ErrorContext {
        &self.context
    }

    /// Records the path that the operation attempted.
    pub fn with_fs_path(mut self, path: impl Into<std::path::PathBuf>) -> Self {
        self.context = std::mem::take(&mut self.context).with_fs_path(path);
        self
    }

    /// Records the path after canonical resolution.
    pub fn with_canonical_path(mut self, path: impl Into<std::path::PathBuf>) -> Self {
        self.context = std::mem::take(&mut self.context).with_canonical_path(path);
        self
    }

    /// Records the path of the content cache.
    pub fn with_cache_path(mut self, path: impl Into<std::path::PathBuf>) -> Self {
        self.context = std::mem::take(&mut self.context).with_cache_path(path);
        self
    }

    /// Records the path of the bundle.
    pub fn with_bundle_path(mut self, path: impl Into<std::path::PathBuf>) -> Self {
        self.context = std::mem::take(&mut self.context).with_bundle_path(path);
        self
    }

    /// Records the name of the bundle member.
    pub fn with_member(mut self, member: impl Into<String>) -> Self {
        self.context = std::mem::take(&mut self.context).with_member(member);
        self
    }

    /// Records the name of the module under load.
    pub fn with_module(mut self, name: impl Into<String>) -> Self {
        self.context = std::mem::take(&mut self.context).with_module(name);
        self
    }

    /// Records the path of the active scope.
    pub fn with_scope(mut self, path: impl Into<std::path::PathBuf>) -> Self {
        self.context = std::mem::take(&mut self.context).with_scope(path);
        self
    }

    /// Records the path of the profile file.
    pub fn with_profile(mut self, path: impl Into<std::path::PathBuf>) -> Self {
        self.context = std::mem::take(&mut self.context).with_profile(path);
        self
    }

    /// Records the path of the project folder.
    pub fn with_project(mut self, path: impl Into<std::path::PathBuf>) -> Self {
        self.context = std::mem::take(&mut self.context).with_project(path);
        self
    }

    /// Records the route of the open document.
    pub fn with_document(mut self, route: impl Into<String>) -> Self {
        self.context = std::mem::take(&mut self.context).with_document(route);
        self
    }

    /// Records the id used for the lookup.
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.context = std::mem::take(&mut self.context).with_id(id);
        self
    }

    /// Records the candidates near the wanted name.
    pub fn with_suggestions(mut self, suggestions: Vec<String>) -> Self {
        self.context = std::mem::take(&mut self.context).with_suggestions(suggestions);
        self
    }

    /// Records the url of the request.
    pub fn with_url(mut self, url: impl Into<String>) -> Self {
        self.context = std::mem::take(&mut self.context).with_url(url);
        self
    }

    /// Records the cycle in the require chain.
    pub fn with_cycle(mut self, chain: Vec<String>) -> Self {
        self.context = std::mem::take(&mut self.context).with_cycle(chain);
        self
    }

    /// Records the argv of the hook.
    pub fn with_argv(mut self, argv: impl Into<String>) -> Self {
        self.context = std::mem::take(&mut self.context).with_argv(argv);
        self
    }

    /// Records the name of the stored state.
    pub fn with_state(mut self, state: impl Into<String>) -> Self {
        self.context = std::mem::take(&mut self.context).with_state(state);
        self
    }

    /// Records the name of the command.
    pub fn with_command(mut self, command: impl Into<String>) -> Self {
        self.context = std::mem::take(&mut self.context).with_command(command);
        self
    }

    /// Records the flags that conflict.
    pub fn with_flags(mut self, flags: Vec<String>) -> Self {
        self.context = std::mem::take(&mut self.context).with_flags(flags);
        self
    }

    /// Records the detail from the backend.
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.context = std::mem::take(&mut self.context).with_detail(detail);
        self
    }

    /// Records the lines of the stack trace.
    pub fn with_trace(mut self, trace: Vec<String>) -> Self {
        self.context = std::mem::take(&mut self.context).with_trace(trace);
        self
    }

    /// Adds one document source to the context.
    pub fn push_source(mut self, source: Source) -> Self {
        self.context = std::mem::take(&mut self.context).push_source(source);
        self
    }

    /// Joins one cause with its kind into a terminal error.
    pub fn terminal(self, kind: super::kind::ErrorKind) -> super::terminal::Error {
        super::terminal::Error::new(kind, self)
    }
}

/// A contextual result that takes context at the call site.
pub trait WithContext<T> {
    /// Records the path that the operation attempted.
    fn with_fs_path(self, path: impl Into<std::path::PathBuf>) -> ContextualResult<T>;
    /// Records the path after canonical resolution.
    fn with_canonical_path(self, path: impl Into<std::path::PathBuf>) -> ContextualResult<T>;
    /// Records the path of the content cache.
    fn with_cache_path(self, path: impl Into<std::path::PathBuf>) -> ContextualResult<T>;
    /// Records the path of the bundle.
    fn with_bundle_path(self, path: impl Into<std::path::PathBuf>) -> ContextualResult<T>;
    /// Records the name of the bundle member.
    fn with_member(self, member: impl Into<String>) -> ContextualResult<T>;
    /// Records the name of the module under load.
    fn with_module(self, name: impl Into<String>) -> ContextualResult<T>;
    /// Records the path of the active scope.
    fn with_scope(self, path: impl Into<std::path::PathBuf>) -> ContextualResult<T>;
    /// Records the path of the profile file.
    fn with_profile(self, path: impl Into<std::path::PathBuf>) -> ContextualResult<T>;
    /// Records the path of the project folder.
    fn with_project(self, path: impl Into<std::path::PathBuf>) -> ContextualResult<T>;
    /// Records the route of the open document.
    fn with_document(self, route: impl Into<String>) -> ContextualResult<T>;
    /// Records the id used for the lookup.
    fn with_id(self, id: impl Into<String>) -> ContextualResult<T>;
    /// Records the candidates near the wanted name.
    fn with_suggestions(self, suggestions: Vec<String>) -> ContextualResult<T>;
    /// Records the url of the request.
    fn with_url(self, url: impl Into<String>) -> ContextualResult<T>;
    /// Records the cycle in the require chain.
    fn with_cycle(self, chain: Vec<String>) -> ContextualResult<T>;
    /// Records the argv of the hook.
    fn with_argv(self, argv: impl Into<String>) -> ContextualResult<T>;
    /// Records the name of the stored state.
    fn with_state(self, state: impl Into<String>) -> ContextualResult<T>;
    /// Records the name of the command.
    fn with_command(self, command: impl Into<String>) -> ContextualResult<T>;
    /// Records the flags that conflict.
    fn with_flags(self, flags: Vec<String>) -> ContextualResult<T>;
    /// Records the detail from the backend.
    fn with_detail(self, detail: impl Into<String>) -> ContextualResult<T>;
    /// Records the lines of the stack trace.
    fn with_trace(self, trace: Vec<String>) -> ContextualResult<T>;
    /// Adds one document source to the context.
    fn push_source(self, source: Source) -> ContextualResult<T>;
}

impl<T> WithContext<T> for ContextualResult<T> {
    /// Records the path that the operation attempted.
    fn with_fs_path(self, path: impl Into<std::path::PathBuf>) -> ContextualResult<T> {
        self.map_err(|contextual| contextual.with_fs_path(path))
    }
    /// Records the path after canonical resolution.
    fn with_canonical_path(self, path: impl Into<std::path::PathBuf>) -> ContextualResult<T> {
        self.map_err(|contextual| contextual.with_canonical_path(path))
    }
    /// Records the path of the content cache.
    fn with_cache_path(self, path: impl Into<std::path::PathBuf>) -> ContextualResult<T> {
        self.map_err(|contextual| contextual.with_cache_path(path))
    }
    /// Records the path of the bundle.
    fn with_bundle_path(self, path: impl Into<std::path::PathBuf>) -> ContextualResult<T> {
        self.map_err(|contextual| contextual.with_bundle_path(path))
    }
    /// Records the name of the bundle member.
    fn with_member(self, member: impl Into<String>) -> ContextualResult<T> {
        self.map_err(|contextual| contextual.with_member(member))
    }
    /// Records the name of the module under load.
    fn with_module(self, name: impl Into<String>) -> ContextualResult<T> {
        self.map_err(|contextual| contextual.with_module(name))
    }
    /// Records the path of the active scope.
    fn with_scope(self, path: impl Into<std::path::PathBuf>) -> ContextualResult<T> {
        self.map_err(|contextual| contextual.with_scope(path))
    }
    /// Records the path of the profile file.
    fn with_profile(self, path: impl Into<std::path::PathBuf>) -> ContextualResult<T> {
        self.map_err(|contextual| contextual.with_profile(path))
    }
    /// Records the path of the project folder.
    fn with_project(self, path: impl Into<std::path::PathBuf>) -> ContextualResult<T> {
        self.map_err(|contextual| contextual.with_project(path))
    }
    /// Records the route of the open document.
    fn with_document(self, route: impl Into<String>) -> ContextualResult<T> {
        self.map_err(|contextual| contextual.with_document(route))
    }
    /// Records the id used for the lookup.
    fn with_id(self, id: impl Into<String>) -> ContextualResult<T> {
        self.map_err(|contextual| contextual.with_id(id))
    }
    /// Records the candidates near the wanted name.
    fn with_suggestions(self, suggestions: Vec<String>) -> ContextualResult<T> {
        self.map_err(|contextual| contextual.with_suggestions(suggestions))
    }
    /// Records the url of the request.
    fn with_url(self, url: impl Into<String>) -> ContextualResult<T> {
        self.map_err(|contextual| contextual.with_url(url))
    }
    /// Records the cycle in the require chain.
    fn with_cycle(self, chain: Vec<String>) -> ContextualResult<T> {
        self.map_err(|contextual| contextual.with_cycle(chain))
    }
    /// Records the argv of the hook.
    fn with_argv(self, argv: impl Into<String>) -> ContextualResult<T> {
        self.map_err(|contextual| contextual.with_argv(argv))
    }
    /// Records the name of the stored state.
    fn with_state(self, state: impl Into<String>) -> ContextualResult<T> {
        self.map_err(|contextual| contextual.with_state(state))
    }
    /// Records the name of the command.
    fn with_command(self, command: impl Into<String>) -> ContextualResult<T> {
        self.map_err(|contextual| contextual.with_command(command))
    }
    /// Records the flags that conflict.
    fn with_flags(self, flags: Vec<String>) -> ContextualResult<T> {
        self.map_err(|contextual| contextual.with_flags(flags))
    }
    /// Records the detail from the backend.
    fn with_detail(self, detail: impl Into<String>) -> ContextualResult<T> {
        self.map_err(|contextual| contextual.with_detail(detail))
    }
    /// Records the lines of the stack trace.
    fn with_trace(self, trace: Vec<String>) -> ContextualResult<T> {
        self.map_err(|contextual| contextual.with_trace(trace))
    }
    /// Adds one document source to the context.
    fn push_source(self, source: Source) -> ContextualResult<T> {
        self.map_err(|contextual| contextual.push_source(source))
    }
}

impl std::fmt::Display for ContextualCause {
    /// Prints one cause with its log object.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.context.is_empty() {
            return write!(formatter, "{}", self.cause);
        }
        write!(formatter, "{} {}", self.cause, self.context)
    }
}

impl std::error::Error for ContextualCause {
    /// Returns the cause.
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.cause)
    }
}

impl<T> From<T> for ContextualCause
where
    T: Into<ErrorCause>,
{
    /// Turns one cause-like value into a contextual cause.
    fn from(cause: T) -> Self {
        Self::new(cause.into())
    }
}

impl From<mlua::Error> for ContextualCause {
    /// Turns one Lua failure into a contextual cause.
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
