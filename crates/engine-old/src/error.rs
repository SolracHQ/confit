//! Error
//!
//! Evaluation failure vocabulary with scope data.

use std::path::PathBuf;

use thiserror::Error;

use confit_model::error::Error as ModelError;
use confit_store::archive::error::ArchiveError;
use confit_store::blob::error::BlobError;
use confit_store::fetch::error::FetchError;
use confit_store::resources::error::ResourceError;

/// Structured scope naming one evaluation failure site.
///
/// Scopes carry data alone, never rendered text. The
/// display renders the same prefix the site names.
#[derive(Debug, Clone)]
pub enum Scope {
    /// Lua method path under calling.
    Method {
        /// The static method path.
        method: &'static str,
    },
    /// Lua method call holding its target display.
    Call {
        /// The static method path.
        method: &'static str,
        /// The target display under calling.
        target: String,
    },
    /// Profile file under evaluating.
    Profile {
        /// The profile path under evaluating.
        profile: PathBuf,
    },
    /// Config contribution under reading.
    Config {
        /// The config name under reading.
        name: String,
    },
    /// Indexed collection entry under reading.
    Item {
        /// The parent scope under reading.
        parent: Box<Scope>,
        /// The collection name under reading.
        collection: &'static str,
        /// The one-based entry index.
        index: usize,
    },
    /// Field segment under reading.
    Slot {
        /// The parent scope under reading.
        parent: Box<Scope>,
        /// The field under reading.
        field: FieldRef,
    },
    /// Map key under descending.
    Key {
        /// The parent scope under descending.
        parent: Box<Scope>,
        /// The map key under descending.
        key: String,
    },
    /// Array position under descending.
    Entry {
        /// The parent scope under descending.
        parent: Box<Scope>,
        /// The one-based position under descending.
        index: usize,
    },
    /// Plugin namespace under loading.
    Plugin {
        /// The plugin user under loading.
        user: String,
        /// The plugin name under loading.
        name: String,
    },
    /// Scoped require owner under resolving.
    Require {
        /// The requiring scope name.
        scope: &'static str,
    },
    /// Path helper under calling.
    PathBase {
        /// The helper name naming the base.
        name: &'static str,
    },
}

impl Scope {
    /// Builds one method scope.
    pub fn method(method: &'static str) -> Self {
        Self::Method { method }
    }

    /// Builds one profile scope.
    pub fn profile(profile: &std::path::Path) -> Self {
        Self::Profile {
            profile: profile.to_path_buf(),
        }
    }

    /// Builds one config scope.
    pub fn config(name: &str) -> Self {
        Self::Config {
            name: name.to_owned(),
        }
    }

    /// Builds one indexed entry scope under this scope.
    pub fn item(&self, collection: &'static str, index: usize) -> Self {
        Self::Item {
            parent: Box::new(self.clone()),
            collection,
            index,
        }
    }

    /// Builds one field segment scope under this scope.
    pub fn slot(&self, field: FieldRef) -> Self {
        Self::Slot {
            parent: Box::new(self.clone()),
            field,
        }
    }

    /// Builds one map key scope under this scope.
    pub fn key(&self, key: &str) -> Self {
        Self::Key {
            parent: Box::new(self.clone()),
            key: key.to_owned(),
        }
    }

    /// Builds one array position scope under this scope.
    pub fn entry(&self, index: usize) -> Self {
        Self::Entry {
            parent: Box::new(self.clone()),
            index,
        }
    }

    /// Builds one plugin scope.
    pub fn plugin(user: &str, name: &str) -> Self {
        Self::Plugin {
            user: user.to_owned(),
            name: name.to_owned(),
        }
    }
}

impl std::fmt::Display for Scope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Method { method } => write!(f, "{method}"),
            Self::Call { method, target } => write!(f, "{method}('{target}')"),
            Self::Profile { profile } => write!(f, "profile '{}'", profile.display()),
            Self::Config { name } => write!(f, "config '{name}'"),
            Self::Item {
                parent,
                collection,
                index,
            } => write!(f, "{parent}: {collection}[{index}]"),
            Self::Slot { parent, field } => write!(f, "{parent}: field '{field}'"),
            Self::Key { parent, key } => write!(f, "{parent}.{key}"),
            Self::Entry { parent, index } => write!(f, "{parent}[{index}]"),
            Self::Plugin { user, name } => write!(f, "confit.plugin.{user}.{name}"),
            Self::Require { scope } => write!(f, "{scope}.require"),
            Self::PathBase { name } => write!(f, "confit.path.{name}"),
        }
    }
}

/// Structured field reference inside one scope.
///
/// Plain names render bare while indexed collections
/// render with their one-based position.
#[derive(Debug, Clone)]
pub enum FieldRef {
    /// Plain field name under reading.
    Name(String),
    /// Indexed collection entry under reading.
    Index {
        /// The collection name under reading.
        collection: &'static str,
        /// The one-based entry index.
        index: usize,
    },
}

impl FieldRef {
    /// Builds one plain field reference.
    pub fn name(field: &str) -> Self {
        Self::Name(field.to_owned())
    }

    /// Builds one indexed field reference.
    pub fn index(collection: &'static str, index: usize) -> Self {
        Self::Index { collection, index }
    }
}

impl std::fmt::Display for FieldRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Name(field) => write!(f, "{field}"),
            Self::Index { collection, index } => write!(f, "{collection}[{index}]"),
        }
    }
}

/// Patch path failure cause.
#[derive(Debug, Clone)]
pub enum PathFault {
    /// Empty path under parsing.
    Empty,
    /// Empty segment under parsing.
    EmptySegment,
    /// Bad index under parsing.
    BadIndex,
}

impl std::fmt::Display for PathFault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(f, "empty path"),
            Self::EmptySegment => write!(f, "empty segment"),
            Self::BadIndex => write!(f, "bad index"),
        }
    }
}

/// Optional hint suffix rendering on its own line.
struct Hint<'a>(Option<&'a str>);

impl std::fmt::Display for Hint<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(hint) = self.0 {
            write!(f, "\nhint: {hint}")?;
        }
        Ok(())
    }
}

/// Engine failure shapes.
#[derive(Debug, Clone, Error)]
pub enum EngineError {
    /// Model failure holding the nested model error.
    #[error(transparent)]
    Model(#[from] ModelError),
    /// Resource failure holding the nested resource error.
    #[error(transparent)]
    Resource(#[from] ResourceError),
    /// Blob failure holding the nested blob error.
    #[error(transparent)]
    Blob(#[from] BlobError),
    /// Fetch failure holding the nested fetch error.
    #[error(transparent)]
    Fetch(#[from] FetchError),
    /// Archive failure holding the nested archive error.
    #[error(transparent)]
    Archive(#[from] ArchiveError),
    /// Unknown failure holding its context with the message.
    #[error("{context}: {message}")]
    Unknown {
        /// The failure context.
        context: String,
        /// The failure message.
        message: String,
    },
    /// Misshaped field holding its scope with the want.
    #[error("{scope}: field '{field}' {want}")]
    Field {
        /// The scope under reading.
        scope: Scope,
        /// The field under reading.
        field: FieldRef,
        /// The wanted shape.
        want: &'static str,
    },
    /// Misshaped scope holding its scope with the want.
    #[error("{scope} {want}")]
    Shape {
        /// The scope under reading.
        scope: Scope,
        /// The wanted shape.
        want: &'static str,
    },
    /// Misshaped scope holding its scope with the want.
    #[error("{scope}: {want}")]
    Detail {
        /// The scope under reading.
        scope: Scope,
        /// The wanted shape.
        want: &'static str,
    },
    /// Failed value render holding its scope with the reason.
    #[error("{scope}: value failed to render: {reason}")]
    RenderDetail {
        /// The scope under rendering.
        scope: Scope,
        /// The nested failure reason.
        reason: String,
    },
    /// Unreadable entry holding its scope with the reason.
    #[error("{scope}: {collection}[{index}] unreadable: {reason}")]
    Item {
        /// The scope under reading.
        scope: Scope,
        /// The collection name under reading.
        collection: &'static str,
        /// The one-based entry index.
        index: usize,
        /// The nested failure reason.
        reason: String,
    },
    /// Repeated field entry holding its scope with the name.
    #[error("{scope}: field '{collection}' declares '{item}' more than once")]
    Repeat {
        /// The scope under reading.
        scope: Scope,
        /// The field name under reading.
        collection: &'static str,
        /// The repeated entry name.
        item: String,
    },
    /// Repeated declaration holding its scope with owners.
    #[error("{scope}: document '{item}' is declared more than once ('{first}' plus '{second}')")]
    Duplicate {
        /// The scope under reading.
        scope: Scope,
        /// The repeated document display.
        item: String,
        /// The first owner name.
        first: String,
        /// The second owner name.
        second: String,
    },
    /// Defined-twice config holding its name.
    #[error("confit.config: config '{name}' is already defined")]
    Defined {
        /// The repeated config name.
        name: String,
    },
    /// Blocked write holding its scope with the path.
    #[error("{scope}: cannot write '{path}': '{path}' is blocked")]
    Blocked {
        /// The scope under writing.
        scope: Scope,
        /// The dotted path under writing.
        path: String,
    },
    /// Out-of-bounds write holding its scope with the path.
    #[error("{scope}: cannot {op} '{path}': index out of bounds")]
    Bounds {
        /// The scope under writing.
        scope: Scope,
        /// The write verb under writing.
        op: &'static str,
        /// The dotted path under writing.
        path: String,
    },
    /// Non-list leaf holding its scope with the path.
    #[error("{scope}: cannot append '{path}': '{path}' holds a non-list leaf")]
    NonList {
        /// The scope under appending.
        scope: Scope,
        /// The dotted path under appending.
        path: String,
    },
    /// Bad patch path holding its scope with the cause.
    #[error("{scope}: invalid path '{path}': {fault}")]
    BadPath {
        /// The scope under parsing.
        scope: Scope,
        /// The dotted path under parsing.
        path: String,
        /// The path failure cause.
        fault: PathFault,
    },
    /// Bad call arity holding its scope with the want.
    #[error("{scope} expects {want}")]
    Arity {
        /// The scope under calling.
        scope: Scope,
        /// The wanted argument shape.
        want: &'static str,
    },
    /// Bad operation arity holding its scope with the want.
    #[error("{scope}: '{op}' expects {want}")]
    OpArity {
        /// The scope under calling.
        scope: Scope,
        /// The operation name under calling.
        op: String,
        /// The wanted argument shape.
        want: &'static str,
    },
    /// Unknown shape holding its scope with the name.
    #[error("{scope} unknown {what} '{name}'")]
    UnknownKind {
        /// The scope under reading.
        scope: Scope,
        /// The unknown shape name.
        what: &'static str,
        /// The unknown value.
        name: String,
    },
    /// Unknown bare shape holding its scope.
    #[error("{scope} unknown {what}")]
    UnknownBare {
        /// The scope under reading.
        scope: Scope,
        /// The unknown shape name.
        what: &'static str,
    },
    /// Unknown field holding its scope with the name.
    #[error("{scope} holds unknown field '{name}'")]
    UnknownField {
        /// The scope under reading.
        scope: Scope,
        /// The unknown field name.
        name: String,
    },
    /// Unknown option holding its scope with the name.
    #[error("{scope}: field '{field}' unknown field '{name}'")]
    OptUnknown {
        /// The scope under reading.
        scope: Scope,
        /// The option table field under reading.
        field: FieldRef,
        /// The unknown field name.
        name: String,
    },
    /// Failed gate call holding its scope with the reason.
    #[error("{scope}: field '{field}' failed: {reason}")]
    GateFailed {
        /// The scope under calling.
        scope: Scope,
        /// The gate field under calling.
        field: FieldRef,
        /// The nested failure reason.
        reason: String,
    },
    /// Bad duration holding its scope with the text.
    #[error("{scope}: field 'timeout' invalid duration '{text}'")]
    Duration {
        /// The scope under reading.
        scope: Scope,
        /// The offending duration text.
        text: String,
    },
    /// Missing plugin holding its user with the name.
    #[error("confit.plugin.{user}.{name}: cannot read 'plugin.lua': no such plugin")]
    MissingPlugin {
        /// The plugin user under loading.
        user: String,
        /// The plugin name under loading.
        name: String,
    },
    /// Bad plugin path holding its user with the name.
    #[error("confit.plugin.{user}.{name}: cannot read 'plugin.lua': bad path")]
    BadPluginPath {
        /// The plugin user under loading.
        user: String,
        /// The plugin name under loading.
        name: String,
    },
    /// Failed plugin read holding its scope with the reason.
    #[error("{scope}: cannot read 'plugin.lua': {reason}")]
    PluginRead {
        /// The plugin scope under loading.
        scope: Scope,
        /// The nested failure reason.
        reason: String,
    },
    /// Raised plugin failure holding its scope with text.
    #[error("{scope}: {message}")]
    Raise {
        /// The plugin scope under raising.
        scope: Scope,
        /// The raised message text.
        message: String,
    },
    /// Unknown callback field holding its scope with the name.
    #[error("{scope}: callback table unknown field '{name}'")]
    CallbackUnknown {
        /// The scope under reading.
        scope: Scope,
        /// The unknown field name.
        name: String,
    },
    /// Bad utf8 body holding its scope with the reason.
    #[error("{scope}: {role} '{target}' holds invalid utf8: {reason}")]
    Utf8 {
        /// The scope under reading.
        scope: Scope,
        /// The body role under reading.
        role: &'static str,
        /// The body identity under reading.
        target: String,
        /// The nested failure reason.
        reason: String,
    },
    /// Missing file name holding its scope with the path.
    #[error("{scope}: path '{path}' holds no file name", path = path.display())]
    FileName {
        /// The scope under reading.
        scope: Scope,
        /// The path under reading.
        path: PathBuf,
    },
    /// Bad digest holding its scope with the reason.
    #[error("{scope}: field 'sha256' holds {reason}")]
    ShaHolds {
        /// The scope under reading.
        scope: Scope,
        /// The nested failure reason.
        reason: String,
    },
    /// Unknown changed document holding its route display.
    #[error("confit.runtime.changed: unknown document '{route}'")]
    ChangedUnknown {
        /// The unknown document display.
        route: String,
    },
    /// Bad destination holding its scope with the want.
    #[error("{scope}: destination '{path}' {want}")]
    Destination {
        /// The scope under reading.
        scope: Scope,
        /// The destination path under reading.
        path: String,
        /// The wanted shape.
        want: &'static str,
    },
    /// Bad mode text holding its scope with the want.
    #[error("{scope}: invalid '{text}': want {want}")]
    ModeBits {
        /// The scope under parsing.
        scope: Scope,
        /// The offending mode text.
        text: String,
        /// The wanted shape.
        want: &'static str,
    },
    /// Unknown rc section holding its scope with the name.
    #[error("{scope}: unknown section '{section}' (expected 'profile', 'config', or 'final')")]
    Section {
        /// The scope under reading.
        scope: Scope,
        /// The unknown section name.
        section: String,
    },
    /// Misshaped rc section holding its scope with the want.
    #[error("{scope}: section '{section}' {want}")]
    SectionLeaf {
        /// The scope under reading.
        scope: Scope,
        /// The section name under reading.
        section: String,
        /// The wanted shape.
        want: &'static str,
    },
    /// Format mismatch holding its target display.
    #[error(
        "confit.patch.structured('{target}'): cannot merge document at '{target}': format mismatch"
    )]
    Mismatch {
        /// The target display under merging.
        target: String,
    },
    /// Missing patch format holding its target display.
    #[error(
        "confit.patch.structured('{target}'): patch for '{target}' holds no format (structured patches name one)"
    )]
    NoFormat {
        /// The target display under merging.
        target: String,
    },
    /// Missing patch object holding its target display.
    #[error("confit.patch.structured('{target}'): patch for '{target}' holds no object")]
    NoObject {
        /// The target display under merging.
        target: String,
    },
    /// Escaping module holding its scope with the request.
    #[error("{scope}: module '{request}' escapes its root")]
    Escape {
        /// The scope under resolving.
        scope: Scope,
        /// The module request under resolving.
        request: String,
    },
    /// Bad rc entry holding its scope with the section.
    #[error("{scope}: invalid rc entry for section '{section}'")]
    RcEntry {
        /// The scope under reading.
        scope: Scope,
        /// The section name under reading.
        section: String,
    },
    /// Dangling require edge holding its owners with the hint.
    #[error("config \"{name}\" requires \"{target}\" config{hint}", hint = Hint(hint.as_deref()))]
    Require {
        /// The requiring config name.
        name: String,
        /// The missing required config name.
        target: String,
        /// The remediation hint, None while absent.
        hint: Option<String>,
    },
    /// Failed slot render holding its scope with the reason.
    #[error("{scope}: field '{field}' failed to render: {reason}")]
    SlotRender {
        /// The scope under rendering.
        scope: Scope,
        /// The field under rendering.
        field: FieldRef,
        /// The nested failure reason.
        reason: String,
    },
    /// Nested field failure holding its scope with the reason.
    #[error("{scope}: field '{field}' {reason}")]
    NestField {
        /// The scope under reading.
        scope: Scope,
        /// The field under reading.
        field: FieldRef,
        /// The nested failure reason.
        reason: String,
    },
    /// Misshaped inner field holding its scope with the want.
    #[error("{scope} field '{field}' {want}")]
    InnerField {
        /// The scope under reading.
        scope: Scope,
        /// The field under reading.
        field: FieldRef,
        /// The wanted shape.
        want: &'static str,
    },
    /// Unknown route base holding its scope with the name.
    #[error("{scope} field 'route' holds unknown base '{base}'")]
    RouteBase {
        /// The scope under reading.
        scope: Scope,
        /// The unknown base name.
        base: String,
    },
    /// Nested scope failure holding its scope with the reason.
    #[error("{scope} {reason}")]
    NestBare {
        /// The scope under reading.
        scope: Scope,
        /// The nested failure reason.
        reason: String,
    },
    /// Nested scope failure holding its scope with the reason.
    #[error("{scope}: {reason}")]
    NestScope {
        /// The scope under reading.
        scope: Scope,
        /// The nested failure reason.
        reason: String,
    },
}

impl From<mlua::Error> for EngineError {
    fn from(error: mlua::Error) -> Self {
        match error.downcast_ref::<EngineError>().cloned() {
            Some(engine) => engine,
            None => Self::Unknown {
                context: "lua".to_owned(),
                message: error.to_string(),
            },
        }
    }
}

impl From<EngineError> for mlua::Error {
    fn from(error: EngineError) -> Self {
        log::error!("evaluation failed: {error:?}");
        mlua::Error::external(error)
    }
}

/// Finds one typed engine error walking nested lua causes.
pub(crate) fn find_engine(error: &mlua::Error) -> Option<EngineError> {
    error.downcast_ref::<EngineError>().cloned()
}

/// Maps one Lua failure onto the typed engine error.
pub(crate) fn wrap(error: mlua::Error) -> EngineError {
    error.into()
}

/// Engine result alias.
pub type Result<T> = std::result::Result<T, EngineError>;
