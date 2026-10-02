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
        /// Holds the static method path.
        method: &'static str,
    },
    /// Lua method call holding its target display.
    Call {
        /// Holds the static method path.
        method: &'static str,
        /// Holds the target display under calling.
        target: String,
    },
    /// Profile file under evaluating.
    Profile {
        /// Holds the profile path under evaluating.
        profile: PathBuf,
    },
    /// Config contribution under reading.
    Config {
        /// Holds the config name under reading.
        name: String,
    },
    /// Indexed collection entry under reading.
    Item {
        /// Holds the parent scope under reading.
        parent: Box<Scope>,
        /// Holds the collection name under reading.
        collection: &'static str,
        /// Holds the one-based entry index.
        index: usize,
    },
    /// Field segment under reading.
    Slot {
        /// Holds the parent scope under reading.
        parent: Box<Scope>,
        /// Holds the field under reading.
        field: FieldRef,
    },
    /// Map key under descending.
    Key {
        /// Holds the parent scope under descending.
        parent: Box<Scope>,
        /// Holds the map key under descending.
        key: String,
    },
    /// Array position under descending.
    Entry {
        /// Holds the parent scope under descending.
        parent: Box<Scope>,
        /// Holds the one-based position under descending.
        index: usize,
    },
    /// Plugin namespace under loading.
    Plugin {
        /// Holds the plugin user under loading.
        user: String,
        /// Holds the plugin name under loading.
        name: String,
    },
    /// Scoped require owner under resolving.
    Require {
        /// Holds the requiring scope name.
        scope: &'static str,
    },
    /// Path helper under calling.
    PathBase {
        /// Holds the helper name naming the base.
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
        /// Holds the collection name under reading.
        collection: &'static str,
        /// Holds the one-based entry index.
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
        /// Holds the failure context.
        context: String,
        /// Holds the failure message.
        message: String,
    },
    /// Misshaped field holding its scope with the want.
    #[error("{scope}: field '{field}' {want}")]
    Field {
        /// Holds the scope under reading.
        scope: Scope,
        /// Holds the field under reading.
        field: FieldRef,
        /// Holds the wanted shape.
        want: &'static str,
    },
    /// Misshaped scope holding its scope with the want.
    #[error("{scope} {want}")]
    Shape {
        /// Holds the scope under reading.
        scope: Scope,
        /// Holds the wanted shape.
        want: &'static str,
    },
    /// Misshaped scope holding its scope with the want.
    #[error("{scope}: {want}")]
    Detail {
        /// Holds the scope under reading.
        scope: Scope,
        /// Holds the wanted shape.
        want: &'static str,
    },
    /// Failed value render holding its scope with the reason.
    #[error("{scope}: value failed to render: {reason}")]
    RenderDetail {
        /// Holds the scope under rendering.
        scope: Scope,
        /// Holds the nested failure reason.
        reason: String,
    },
    /// Unreadable entry holding its scope with the reason.
    #[error("{scope}: {collection}[{index}] unreadable: {reason}")]
    Item {
        /// Holds the scope under reading.
        scope: Scope,
        /// Holds the collection name under reading.
        collection: &'static str,
        /// Holds the one-based entry index.
        index: usize,
        /// Holds the nested failure reason.
        reason: String,
    },
    /// Repeated field entry holding its scope with the name.
    #[error("{scope}: field '{collection}' declares '{item}' more than once")]
    Repeat {
        /// Holds the scope under reading.
        scope: Scope,
        /// Holds the field name under reading.
        collection: &'static str,
        /// Holds the repeated entry name.
        item: String,
    },
    /// Repeated declaration holding its scope with owners.
    #[error("{scope}: document '{item}' is declared more than once ('{first}' plus '{second}')")]
    Duplicate {
        /// Holds the scope under reading.
        scope: Scope,
        /// Holds the repeated document display.
        item: String,
        /// Holds the first owner name.
        first: String,
        /// Holds the second owner name.
        second: String,
    },
    /// Defined-twice config holding its name.
    #[error("confit.config: config '{name}' is already defined")]
    Defined {
        /// Holds the repeated config name.
        name: String,
    },
    /// Blocked write holding its scope with the path.
    #[error("{scope}: cannot write '{path}': '{path}' is blocked")]
    Blocked {
        /// Holds the scope under writing.
        scope: Scope,
        /// Holds the dotted path under writing.
        path: String,
    },
    /// Out-of-bounds write holding its scope with the path.
    #[error("{scope}: cannot {op} '{path}': index out of bounds")]
    Bounds {
        /// Holds the scope under writing.
        scope: Scope,
        /// Holds the write verb under writing.
        op: &'static str,
        /// Holds the dotted path under writing.
        path: String,
    },
    /// Non-list leaf holding its scope with the path.
    #[error("{scope}: cannot append '{path}': '{path}' holds a non-list leaf")]
    NonList {
        /// Holds the scope under appending.
        scope: Scope,
        /// Holds the dotted path under appending.
        path: String,
    },
    /// Bad patch path holding its scope with the cause.
    #[error("{scope}: invalid path '{path}': {fault}")]
    BadPath {
        /// Holds the scope under parsing.
        scope: Scope,
        /// Holds the dotted path under parsing.
        path: String,
        /// Holds the path failure cause.
        fault: PathFault,
    },
    /// Bad call arity holding its scope with the want.
    #[error("{scope} expects {want}")]
    Arity {
        /// Holds the scope under calling.
        scope: Scope,
        /// Holds the wanted argument shape.
        want: &'static str,
    },
    /// Bad operation arity holding its scope with the want.
    #[error("{scope}: '{op}' expects {want}")]
    OpArity {
        /// Holds the scope under calling.
        scope: Scope,
        /// Holds the operation name under calling.
        op: String,
        /// Holds the wanted argument shape.
        want: &'static str,
    },
    /// Unknown shape holding its scope with the name.
    #[error("{scope} unknown {what} '{name}'")]
    UnknownKind {
        /// Holds the scope under reading.
        scope: Scope,
        /// Holds the unknown shape name.
        what: &'static str,
        /// Holds the unknown value.
        name: String,
    },
    /// Unknown bare shape holding its scope.
    #[error("{scope} unknown {what}")]
    UnknownBare {
        /// Holds the scope under reading.
        scope: Scope,
        /// Holds the unknown shape name.
        what: &'static str,
    },
    /// Unknown field holding its scope with the name.
    #[error("{scope} holds unknown field '{name}'")]
    UnknownField {
        /// Holds the scope under reading.
        scope: Scope,
        /// Holds the unknown field name.
        name: String,
    },
    /// Unknown option holding its scope with the name.
    #[error("{scope}: field '{field}' unknown field '{name}'")]
    OptUnknown {
        /// Holds the scope under reading.
        scope: Scope,
        /// Holds the option table field under reading.
        field: FieldRef,
        /// Holds the unknown field name.
        name: String,
    },
    /// Failed gate call holding its scope with the reason.
    #[error("{scope}: field '{field}' failed: {reason}")]
    GateFailed {
        /// Holds the scope under calling.
        scope: Scope,
        /// Holds the gate field under calling.
        field: FieldRef,
        /// Holds the nested failure reason.
        reason: String,
    },
    /// Bad duration holding its scope with the text.
    #[error("{scope}: field 'timeout' invalid duration '{text}'")]
    Duration {
        /// Holds the scope under reading.
        scope: Scope,
        /// Holds the offending duration text.
        text: String,
    },
    /// Missing plugin holding its user with the name.
    #[error("confit.plugin.{user}.{name}: cannot read 'plugin.lua': no such plugin")]
    MissingPlugin {
        /// Holds the plugin user under loading.
        user: String,
        /// Holds the plugin name under loading.
        name: String,
    },
    /// Bad plugin path holding its user with the name.
    #[error("confit.plugin.{user}.{name}: cannot read 'plugin.lua': bad path")]
    BadPluginPath {
        /// Holds the plugin user under loading.
        user: String,
        /// Holds the plugin name under loading.
        name: String,
    },
    /// Failed plugin read holding its scope with the reason.
    #[error("{scope}: cannot read 'plugin.lua': {reason}")]
    PluginRead {
        /// Holds the plugin scope under loading.
        scope: Scope,
        /// Holds the nested failure reason.
        reason: String,
    },
    /// Raised plugin failure holding its scope with text.
    #[error("{scope}: {message}")]
    Raise {
        /// Holds the plugin scope under raising.
        scope: Scope,
        /// Holds the raised message text.
        message: String,
    },
    /// Unknown callback field holding its scope with the name.
    #[error("{scope}: callback table unknown field '{name}'")]
    CallbackUnknown {
        /// Holds the scope under reading.
        scope: Scope,
        /// Holds the unknown field name.
        name: String,
    },
    /// Bad utf8 body holding its scope with the reason.
    #[error("{scope}: {role} '{target}' holds invalid utf8: {reason}")]
    Utf8 {
        /// Holds the scope under reading.
        scope: Scope,
        /// Holds the body role under reading.
        role: &'static str,
        /// Holds the body identity under reading.
        target: String,
        /// Holds the nested failure reason.
        reason: String,
    },
    /// Missing file name holding its scope with the path.
    #[error("{scope}: path '{path}' holds no file name", path = path.display())]
    FileName {
        /// Holds the scope under reading.
        scope: Scope,
        /// Holds the path under reading.
        path: PathBuf,
    },
    /// Bad digest holding its scope with the reason.
    #[error("{scope}: field 'sha256' holds {reason}")]
    ShaHolds {
        /// Holds the scope under reading.
        scope: Scope,
        /// Holds the nested failure reason.
        reason: String,
    },
    /// Unknown changed document holding its route display.
    #[error("confit.runtime.changed: unknown document '{route}'")]
    ChangedUnknown {
        /// Holds the unknown document display.
        route: String,
    },
    /// Bad destination holding its scope with the want.
    #[error("{scope}: destination '{path}' {want}")]
    Destination {
        /// Holds the scope under reading.
        scope: Scope,
        /// Holds the destination path under reading.
        path: String,
        /// Holds the wanted shape.
        want: &'static str,
    },
    /// Bad mode text holding its scope with the want.
    #[error("{scope}: invalid '{text}': want {want}")]
    ModeBits {
        /// Holds the scope under parsing.
        scope: Scope,
        /// Holds the offending mode text.
        text: String,
        /// Holds the wanted shape.
        want: &'static str,
    },
    /// Unknown rc section holding its scope with the name.
    #[error("{scope}: unknown section '{section}' (expected 'profile', 'config', or 'final')")]
    Section {
        /// Holds the scope under reading.
        scope: Scope,
        /// Holds the unknown section name.
        section: String,
    },
    /// Misshaped rc section holding its scope with the want.
    #[error("{scope}: section '{section}' {want}")]
    SectionLeaf {
        /// Holds the scope under reading.
        scope: Scope,
        /// Holds the section name under reading.
        section: String,
        /// Holds the wanted shape.
        want: &'static str,
    },
    /// Format mismatch holding its target display.
    #[error(
        "confit.patch.structured('{target}'): cannot merge document at '{target}': format mismatch"
    )]
    Mismatch {
        /// Holds the target display under merging.
        target: String,
    },
    /// Missing patch format holding its target display.
    #[error(
        "confit.patch.structured('{target}'): patch for '{target}' holds no format (structured patches name one)"
    )]
    NoFormat {
        /// Holds the target display under merging.
        target: String,
    },
    /// Missing patch object holding its target display.
    #[error("confit.patch.structured('{target}'): patch for '{target}' holds no object")]
    NoObject {
        /// Holds the target display under merging.
        target: String,
    },
    /// Escaping module holding its scope with the request.
    #[error("{scope}: module '{request}' escapes its root")]
    Escape {
        /// Holds the scope under resolving.
        scope: Scope,
        /// Holds the module request under resolving.
        request: String,
    },
    /// Bad rc entry holding its scope with the section.
    #[error("{scope}: invalid rc entry for section '{section}'")]
    RcEntry {
        /// Holds the scope under reading.
        scope: Scope,
        /// Holds the section name under reading.
        section: String,
    },
    /// Dangling require edge holding its owners with the hint.
    #[error("config \"{name}\" requires \"{target}\" config{hint}", hint = Hint(hint.as_deref()))]
    Require {
        /// Holds the requiring config name.
        name: String,
        /// Holds the missing required config name.
        target: String,
        /// Holds the remediation hint, None while absent.
        hint: Option<String>,
    },
    /// Failed slot render holding its scope with the reason.
    #[error("{scope}: field '{field}' failed to render: {reason}")]
    SlotRender {
        /// Holds the scope under rendering.
        scope: Scope,
        /// Holds the field under rendering.
        field: FieldRef,
        /// Holds the nested failure reason.
        reason: String,
    },
    /// Nested field failure holding its scope with the reason.
    #[error("{scope}: field '{field}' {reason}")]
    NestField {
        /// Holds the scope under reading.
        scope: Scope,
        /// Holds the field under reading.
        field: FieldRef,
        /// Holds the nested failure reason.
        reason: String,
    },
    /// Misshaped inner field holding its scope with the want.
    #[error("{scope} field '{field}' {want}")]
    InnerField {
        /// Holds the scope under reading.
        scope: Scope,
        /// Holds the field under reading.
        field: FieldRef,
        /// Holds the wanted shape.
        want: &'static str,
    },
    /// Unknown route base holding its scope with the name.
    #[error("{scope} field 'route' holds unknown base '{base}'")]
    RouteBase {
        /// Holds the scope under reading.
        scope: Scope,
        /// Holds the unknown base name.
        base: String,
    },
    /// Nested scope failure holding its scope with the reason.
    #[error("{scope} {reason}")]
    NestBare {
        /// Holds the scope under reading.
        scope: Scope,
        /// Holds the nested failure reason.
        reason: String,
    },
    /// Nested scope failure holding its scope with the reason.
    #[error("{scope}: {reason}")]
    NestScope {
        /// Holds the scope under reading.
        scope: Scope,
        /// Holds the nested failure reason.
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
