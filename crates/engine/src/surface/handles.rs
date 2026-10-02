//! Handles
//!
//! Typed Lua handles over core identity and the fetch constructor.

use std::io::Read as _;

use mlua::Value;
use mlua_extras::{TypedUserData, typeduserdata_impl};
use serde_json::Value as Json;

use super::confit_table;
use super::document::{check_rel, mode_bits, tree_table};
use crate::error::{EngineError, FieldRef, Scope};
use crate::lua::{JsonExt, ValueExt};
use crate::model::TreeMemberDecl;
use confit_model::routes::Route;
use confit_model::sha::Sha;
use confit_store::Stores;
use confit_store::blob::BlobSource;
use confit_store::handles::{
    ArchiveHandle, BlobHandle, FetchHandle, ResourceHandle, TrustedHandle,
};
use confit_store::resources::error::ResourceError;

/// Fetch userdata returned by the fetch constructor.
///
/// Lua mapping over the core fetch identity. The cache file
/// path, the content hash, and the origin url travel together.
/// Store access rides along for text and tree verbs.
#[derive(Clone, TypedUserData)]
pub(crate) struct LuaFetchHandle {
    /// Core fetch identity under wrapping.
    #[lua(skip)]
    handle: FetchHandle,
    /// Stores behind text and tree verbs.
    #[lua(skip)]
    stores: Stores,
}

/// Resource userdata for exec-rooted project files and archive members.
///
/// Lua mapping over the core resource identity. Member
/// handles carry their archive for byte reads; project
/// handles read through the workspace instead.
#[derive(Clone, TypedUserData)]
pub(crate) struct LuaResourceHandle {
    /// Core resource identity under wrapping.
    #[lua(skip)]
    handle: ResourceHandle,
    /// Stores behind text and tree verbs.
    #[lua(skip)]
    stores: Stores,
    /// Archive holding member bytes, holding `None` for project files.
    #[lua(skip)]
    archive: Option<ArchiveHandle>,
}

/// Archive userdata for verified compressed sources.
///
/// Lua mapping over the core archive identity. Verbs arrive
/// through seal-then-act helpers shared with source handles.
#[derive(Clone, TypedUserData)]
pub(crate) struct LuaArchiveHandle {
    /// Core archive identity under wrapping.
    #[lua(skip)]
    handle: ArchiveHandle,
    /// Stores behind member verbs.
    #[lua(skip)]
    stores: Stores,
}

/// Route userdata for late-bound destinations.
///
/// Lua mapping over the core destination route. Constructors
/// take routes where a location is meant.
#[derive(Clone, TypedUserData)]
pub(crate) struct LuaRoute {
    /// Core destination route under wrapping.
    #[lua(skip)]
    handle: Route,
}

/// Blob userdata for sealed content-addressed bytes.
///
/// Lua mapping over the core blob identity. The content hash
/// and the stored hash travel together from the blob pool.
#[derive(Clone, TypedUserData)]
pub(crate) struct LuaBlobHandle {
    /// Core blob identity under wrapping.
    #[lua(skip)]
    handle: BlobHandle,
}

impl LuaFetchHandle {
    /// Builds fetch userdata holding store access.
    pub(crate) fn new(handle: FetchHandle, stores: Stores) -> Self {
        Self { handle, stores }
    }
}

impl LuaResourceHandle {
    /// Builds resource userdata holding store access.
    pub(crate) fn new(
        handle: ResourceHandle,
        stores: Stores,
        archive: Option<ArchiveHandle>,
    ) -> Self {
        Self {
            handle,
            stores,
            archive,
        }
    }
}

impl LuaArchiveHandle {
    /// Builds archive userdata holding store access.
    pub(crate) fn new(handle: ArchiveHandle, stores: Stores) -> Self {
        Self { handle, stores }
    }
}

impl From<Route> for LuaRoute {
    fn from(handle: Route) -> Self {
        Self { handle }
    }
}

impl LuaRoute {
    /// Reads the core destination route.
    pub(crate) fn core(&self) -> &Route {
        &self.handle
    }
}

impl LuaBlobHandle {
    /// Builds blob userdata holding sealed pool identity.
    pub(crate) fn new(handle: BlobHandle) -> Self {
        Self { handle }
    }

    /// Reads the core blob identity.
    pub(crate) fn core(&self) -> &BlobHandle {
        &self.handle
    }
}

/// Seals one trusted source as a verified archive.
///
/// # Errors
///
/// - [`EngineError::Archive`] for non-archive sources.
fn seal(source: &dyn TrustedHandle, stores: &Stores) -> mlua::Result<ArchiveHandle> {
    Ok(stores
        .archives()
        .archive(source)
        .map_err(EngineError::from)?)
}

/// Reads member bytes from their archive spill.
///
/// # Errors
///
/// - [`EngineError::Resource`] for unreadable members.
fn member_bytes(member: &ResourceHandle, stores: &Stores) -> mlua::Result<Vec<u8>> {
    let mut reader = stores.resources().open(member).map_err(EngineError::from)?;
    let mut bytes = Vec::new();
    reader
        .read_to_end(&mut bytes)
        .map_err(|error| EngineError::from(ResourceError::from_io(member.canonical(), error)))?;
    Ok(bytes)
}

/// Pools trusted source bytes under content identity.
///
/// # Errors
///
/// - [`EngineError::Blob`] for pool failures.
fn pool_source(stores: &Stores, source: &dyn TrustedHandle) -> mlua::Result<(BlobHandle, u64)> {
    let blobs = stores.blobs();
    let handle = blobs
        .put(BlobSource::Handle(source))
        .map_err(EngineError::from)?;
    let size = blobs.len(&handle).map_err(EngineError::from)?;
    Ok((handle, size))
}

/// Structured document format for handle decode views.
enum DecodeFormat {
    Toml,
    Json,
    Yaml,
}

/// Reads fetch bytes from the fetch cache.
///
/// # Errors
///
/// - [`EngineError::Fetch`] for unreadable cache entries.
fn fetch_bytes(handle: &LuaFetchHandle) -> mlua::Result<Vec<u8>> {
    Ok(handle
        .stores
        .fetch()
        .read(&handle.handle)
        .map_err(EngineError::from)?)
}

/// Reads resource bytes from the workspace or the archive spill.
///
/// # Errors
///
/// - [`EngineError::Resource`] for unreadable files.
fn resource_bytes(handle: &LuaResourceHandle) -> mlua::Result<Vec<u8>> {
    if handle.archive.is_some() {
        return member_bytes(&handle.handle, &handle.stores);
    }
    Ok(handle
        .stores
        .resources()
        .read_text(&handle.handle)
        .map_err(EngineError::from)?
        .into_bytes())
}

/// Decodes handle bytes into a Lua table.
///
/// # Errors
///
/// - [`EngineError::NestScope`] for malformed documents.
/// - [`EngineError::Shape`] for non-table documents.
fn decode_bytes(
    lua: &mlua::Lua,
    bytes: &[u8],
    format: DecodeFormat,
    scope: &Scope,
) -> mlua::Result<mlua::Table> {
    let nest = |reason: String| EngineError::NestScope {
        scope: scope.clone(),
        reason,
    };
    let parsed: Json = match format {
        DecodeFormat::Toml => {
            let text = std::str::from_utf8(bytes).map_err(|error| nest(error.to_string()))?;
            let value: toml::Value =
                toml::from_str(text).map_err(|error| nest(error.to_string()))?;
            serde_json::to_value(&value).map_err(|error| nest(error.to_string()))?
        }
        DecodeFormat::Json => {
            let value: Json =
                serde_json::from_slice(bytes).map_err(|error| nest(error.to_string()))?;
            serde_json::to_value(&value).map_err(|error| nest(error.to_string()))?
        }
        DecodeFormat::Yaml => {
            let text = std::str::from_utf8(bytes).map_err(|error| nest(error.to_string()))?;
            let value: noyalib::Value =
                noyalib::from_str(text).map_err(|error| nest(error.to_string()))?;
            serde_json::to_value(&value).map_err(|error| nest(error.to_string()))?
        }
    };
    match parsed.to_lua(lua, scope)? {
        Value::Table(table) => Ok(table),
        _ => Err(EngineError::Shape {
            scope: scope.clone(),
            want: "document must hold a table",
        }
        .into()),
    }
}

/// Resolves one opaque source value into a blob handle and size.
///
/// Fetch, resource, and archive-member handles pool their
/// bytes once. Blob-shaped values never arrive here; opaque
/// tables carry blob hashes directly.
///
/// # Errors
///
/// - [`EngineError::Field`] for non-handle sources.
/// - [`EngineError::Blob`] for pool failures.
pub(crate) fn blob_for_opaque(
    value: &Value,
    stores: &Stores,
    scope: &Scope,
) -> mlua::Result<(BlobHandle, u64)> {
    let field = || EngineError::Field {
        scope: scope.clone(),
        field: FieldRef::name("src"),
        want: "must be a fetch, resource, or archive member handle",
    };
    let Some(data) = value.as_userdata() else {
        return Err(field().into());
    };
    if let Ok(handle) = data.borrow::<LuaFetchHandle>() {
        return pool_source(stores, &handle.handle);
    }
    if let Ok(handle) = data.borrow::<LuaResourceHandle>() {
        return pool_source(stores, &handle.handle);
    }
    Err(field().into())
}

/// Builds one tree document table from a sealed archive.
///
/// Members pass the callback one by one. Nil skips, true
/// keeps at the same path, a table overrides path and
/// mode. Kept members pool their bytes once and land in
/// relative path order.
///
/// # Errors
///
/// - [`EngineError::Archive`] for unreadable archives.
/// - [`EngineError::Resource`] for unreadable members.
/// - [`EngineError::Blob`] for pool failures.
/// - [`EngineError::Repeat`] for duplicate keeps.
/// - [`EngineError::Detail`] for empty picks.
fn tree_from_archive(
    lua: &mlua::Lua,
    sealed: &ArchiveHandle,
    stores: &Stores,
    destination: Route,
    callback: mlua::Function,
    scope: &Scope,
) -> mlua::Result<mlua::Table> {
    let archives = stores.archives();
    let names = archives.members(sealed).map_err(EngineError::from)?;
    let mut kept: Vec<TreeMemberDecl> = Vec::new();
    for name in &names {
        let member = archives
            .extract_member(sealed, name)
            .map_err(EngineError::from)?;
        let lua_member =
            LuaResourceHandle::new(member.clone(), stores.clone(), Some(sealed.clone()));
        let returned: Value = callback.call(lua_member)?;
        let Some(pick) = parse_tree_pick(&returned, name, scope)? else {
            continue;
        };
        let mode = match pick.mode {
            Some(mode) => mode,
            None => stores
                .resources()
                .mode(&member)
                .map_err(EngineError::from)?,
        };
        if kept.iter().any(|item| item.rel == pick.rel) {
            return Err(EngineError::Repeat {
                scope: scope.clone(),
                collection: "tree",
                item: pick.rel.clone(),
            }
            .into());
        }
        let (blob, size) = pool_source(stores, &member)?;
        kept.push(TreeMemberDecl {
            rel: pick.rel,
            blob,
            size,
            mode,
        });
    }
    if kept.is_empty() {
        return Err(EngineError::Detail {
            scope: scope.clone(),
            want: "tree kept no members: check the pick filter",
        }
        .into());
    }
    kept.sort_by(|left, right| left.rel.cmp(&right.rel));
    tree_table(lua, destination, kept)
}

/// One parsed tree member pick.
struct TreePick {
    /// Destination-relative member path.
    rel: String,
    /// Override mode bits, holding `None` for archive bits.
    mode: Option<u32>,
}

/// Parses one tree callback return into a pick.
///
/// Nil skips. True keeps at the same path. A table
/// overrides path and mode.
///
/// # Errors
///
/// - [`EngineError::Detail`] for misshaped returns.
/// - [`EngineError::CallbackUnknown`] for unknown fields.
/// - [`EngineError::ModeBits`] for bad mode text.
fn parse_tree_pick(returned: &Value, name: &str, scope: &Scope) -> mlua::Result<Option<TreePick>> {
    let shape = EngineError::Detail {
        scope: scope.clone(),
        want: "callback must return nil, true, or a { path, mode } table",
    };
    if returned.is_nil() {
        return Ok(None);
    }
    if let Some(keep) = returned.as_boolean() {
        if keep {
            check_rel(scope, name)?;
            return Ok(Some(TreePick {
                rel: name.to_string(),
                mode: None,
            }));
        }
        return Err(shape.into());
    }
    let Some(table) = returned.as_table() else {
        return Err(shape.into());
    };
    for pair in table.pairs::<Value, Value>() {
        let (key, _) = pair?;
        let Some(field) = key.opt_str() else {
            return Err(EngineError::Detail {
                scope: scope.clone(),
                want: "callback table must hold string keys",
            }
            .into());
        };
        if field != "path" && field != "mode" {
            return Err(EngineError::CallbackUnknown {
                scope: scope.clone(),
                name: field,
            }
            .into());
        }
    }
    let path_value: Value = table.get("path")?;
    let Some(rel) = path_value.opt_str() else {
        return Err(EngineError::Detail {
            scope: scope.clone(),
            want: "callback table field 'path' must be a string",
        }
        .into());
    };
    check_rel(scope, &rel)?;
    let mode_value: Value = table.get("mode")?;
    let mode = if mode_value.is_nil() {
        None
    } else if let Some(bits) = mode_value.as_integer() {
        if !(0..=0o7777).contains(&bits) {
            return Err(EngineError::Detail {
                scope: scope.clone(),
                want: "callback table field 'mode' must hold permission bits",
            }
            .into());
        }
        Some(bits as u32)
    } else if let Some(raw) = mode_value.opt_str() {
        Some(mode_bits(&raw, scope)?)
    } else {
        return Err(EngineError::Detail {
            scope: scope.clone(),
            want: "callback table field 'mode' must be a string or integer",
        }
        .into());
    };
    Ok(Some(TreePick { rel, mode }))
}

#[typeduserdata_impl]
impl LuaFetchHandle {
    /// Decoded body text for the fetched artifact.
    ///
    /// # Errors
    ///
    /// - [`EngineError::Fetch`] for unreadable cache entries.
    /// - [`EngineError::Utf8`] for non-UTF-8 bodies.
    fn text(&self) -> mlua::Result<String> {
        const CALLER: &str = "FetchHandle:text";
        let scope = Scope::method(CALLER);
        let bytes = self
            .stores
            .fetch()
            .read(&self.handle)
            .map_err(EngineError::from)?;
        String::from_utf8(bytes).map_err(|error| {
            EngineError::Utf8 {
                scope: scope.clone(),
                role: "body for",
                target: self.handle.origin().to_owned(),
                reason: error.to_string(),
            }
            .into()
        })
    }

    /// Decoded TOML table for the fetched artifact.
    ///
    /// # Errors
    ///
    /// - [`EngineError::Fetch`] for unreadable cache entries.
    /// - [`EngineError::NestScope`] for malformed documents.
    /// - [`EngineError::Shape`] for non-table documents.
    fn toml(&self, lua: &mlua::Lua) -> mlua::Result<mlua::Table> {
        const CALLER: &str = "FetchHandle:toml";
        let scope = Scope::method(CALLER);
        let bytes = fetch_bytes(self)?;
        decode_bytes(lua, &bytes, DecodeFormat::Toml, &scope)
    }

    /// Decoded JSON table for the fetched artifact.
    ///
    /// # Errors
    ///
    /// - [`EngineError::Fetch`] for unreadable cache entries.
    /// - [`EngineError::NestScope`] for malformed documents.
    /// - [`EngineError::Shape`] for non-table documents.
    fn json(&self, lua: &mlua::Lua) -> mlua::Result<mlua::Table> {
        const CALLER: &str = "FetchHandle:json";
        let scope = Scope::method(CALLER);
        let bytes = fetch_bytes(self)?;
        decode_bytes(lua, &bytes, DecodeFormat::Json, &scope)
    }

    /// Decoded YAML table for the fetched artifact.
    ///
    /// # Errors
    ///
    /// - [`EngineError::Fetch`] for unreadable cache entries.
    /// - [`EngineError::NestScope`] for malformed documents.
    /// - [`EngineError::Shape`] for non-table documents.
    fn yaml(&self, lua: &mlua::Lua) -> mlua::Result<mlua::Table> {
        const CALLER: &str = "FetchHandle:yaml";
        let scope = Scope::method(CALLER);
        let bytes = fetch_bytes(self)?;
        decode_bytes(lua, &bytes, DecodeFormat::Yaml, &scope)
    }

    /// Cache file path for the fetched artifact.
    #[lua(infallible)]
    fn canonical(&self) -> String {
        self.handle.canonical().to_string_lossy().into_owned()
    }

    /// Content hash for the fetched artifact.
    #[lua(infallible)]
    fn sha(&self) -> String {
        self.handle.sha().hex()
    }

    /// Seals the fetched artifact as a verified archive.
    ///
    /// # Errors
    ///
    /// - [`EngineError::Archive`] for non-archive sources.
    fn archive(&self) -> mlua::Result<LuaArchiveHandle> {
        seal(&self.handle, &self.stores)
            .map(|handle| LuaArchiveHandle::new(handle, self.stores.clone()))
    }

    /// Builds one tree document from the fetched archive.
    ///
    /// The callback receives one resource handle per member
    /// and returns nil, true, or a `{ path, mode }` table.
    ///
    /// # Errors
    ///
    /// - [`EngineError::Archive`] for non-archive sources.
    /// - [`EngineError::Field`] for misshaped arguments.
    /// - [`EngineError::Repeat`] for duplicate keeps.
    /// - [`EngineError::Detail`] for empty picks.
    fn tree(&self, lua: &mlua::Lua, args: (Value, Value)) -> mlua::Result<mlua::Table> {
        const CALLER: &str = "FetchHandle:tree";
        let scope = Scope::method(CALLER);
        let (dest, callback) = parse_tree_args(args, &scope)?;
        let sealed = seal(&self.handle, &self.stores)?;
        tree_from_archive(lua, &sealed, &self.stores, dest, callback, &scope)
    }

    /// Picks one archive member by its archive-relative name.
    ///
    /// # Errors
    ///
    /// - [`EngineError::Archive`] for non-archives and
    ///   unknown members.
    fn extract_member(&self, name: String) -> mlua::Result<LuaResourceHandle> {
        let sealed = seal(&self.handle, &self.stores)?;
        let member = self
            .stores
            .archives()
            .extract_member(&sealed, &name)
            .map_err(EngineError::from)?;
        Ok(LuaResourceHandle::new(
            member,
            self.stores.clone(),
            Some(sealed),
        ))
    }

    /// One-line identity for logs and errors.
    #[lua(meta, infallible)]
    fn __tostring(&self) -> String {
        format!(
            "FetchHandle(sha256:{}, origin:{})",
            self.handle.sha(),
            self.handle.origin()
        )
    }
}

#[typeduserdata_impl]
impl LuaResourceHandle {
    /// Decoded file text for the project file or member.
    ///
    /// Project files read through the workspace. Members
    /// stream from their archive spill.
    ///
    /// # Errors
    ///
    /// - [`EngineError::Resource`] for unreadable files.
    /// - [`EngineError::Utf8`] for non-UTF-8 bodies.
    fn text(&self) -> mlua::Result<String> {
        const CALLER: &str = "ResourceHandle:text";
        let scope = Scope::method(CALLER);
        if self.archive.is_none() {
            return Ok(self
                .stores
                .resources()
                .read_text(&self.handle)
                .map_err(EngineError::from)?);
        }
        let bytes = member_bytes(&self.handle, &self.stores)?;
        String::from_utf8(bytes).map_err(|error| {
            EngineError::Utf8 {
                scope: scope.clone(),
                role: "file",
                target: self.handle.canonical().display().to_string(),
                reason: error.to_string(),
            }
            .into()
        })
    }

    /// Decoded TOML table for the project file or member.
    ///
    /// # Errors
    ///
    /// - [`EngineError::Resource`] for unreadable files.
    /// - [`EngineError::NestScope`] for malformed documents.
    /// - [`EngineError::Shape`] for non-table documents.
    fn toml(&self, lua: &mlua::Lua) -> mlua::Result<mlua::Table> {
        const CALLER: &str = "ResourceHandle:toml";
        let scope = Scope::method(CALLER);
        let bytes = resource_bytes(self)?;
        decode_bytes(lua, &bytes, DecodeFormat::Toml, &scope)
    }

    /// Decoded JSON table for the project file or member.
    ///
    /// # Errors
    ///
    /// - [`EngineError::Resource`] for unreadable files.
    /// - [`EngineError::NestScope`] for malformed documents.
    /// - [`EngineError::Shape`] for non-table documents.
    fn json(&self, lua: &mlua::Lua) -> mlua::Result<mlua::Table> {
        const CALLER: &str = "ResourceHandle:json";
        let scope = Scope::method(CALLER);
        let bytes = resource_bytes(self)?;
        decode_bytes(lua, &bytes, DecodeFormat::Json, &scope)
    }

    /// Decoded YAML table for the project file or member.
    ///
    /// # Errors
    ///
    /// - [`EngineError::Resource`] for unreadable files.
    /// - [`EngineError::NestScope`] for malformed documents.
    /// - [`EngineError::Shape`] for non-table documents.
    fn yaml(&self, lua: &mlua::Lua) -> mlua::Result<mlua::Table> {
        const CALLER: &str = "ResourceHandle:yaml";
        let scope = Scope::method(CALLER);
        let bytes = resource_bytes(self)?;
        decode_bytes(lua, &bytes, DecodeFormat::Yaml, &scope)
    }

    /// File name for the resource path.
    ///
    /// # Errors
    ///
    /// - [`EngineError::FileName`] for nameless paths.
    fn name(&self) -> mlua::Result<String> {
        const CALLER: &str = "ResourceHandle:name";
        match self
            .handle
            .canonical()
            .file_name()
            .and_then(|name| name.to_str())
        {
            Some(name) => Ok(name.to_string()),
            None => Err(EngineError::FileName {
                scope: Scope::method(CALLER),
                path: self.handle.canonical().to_path_buf(),
            }
            .into()),
        }
    }

    /// Keeps the member at its own path in tree picks.
    ///
    /// Sugar over the plain true return.
    #[lua(infallible)]
    fn keep(&self) -> bool {
        true
    }

    /// Canonical path for the resource.
    #[lua(infallible)]
    fn canonical(&self) -> String {
        self.handle.canonical().to_string_lossy().into_owned()
    }

    /// Content hash for the resource.
    #[lua(infallible)]
    fn sha(&self) -> String {
        self.handle.sha().hex()
    }

    /// Seals the resource as a verified archive.
    ///
    /// # Errors
    ///
    /// - [`EngineError::Archive`] for non-archive sources.
    fn archive(&self) -> mlua::Result<LuaArchiveHandle> {
        seal(&self.handle, &self.stores)
            .map(|handle| LuaArchiveHandle::new(handle, self.stores.clone()))
    }

    /// Builds one tree document from the resource archive.
    ///
    /// The callback receives one resource handle per member
    /// and returns nil, true, or a `{ path, mode }` table.
    ///
    /// # Errors
    ///
    /// - [`EngineError::Archive`] for non-archive sources.
    /// - [`EngineError::Field`] for misshaped arguments.
    /// - [`EngineError::Repeat`] for duplicate keeps.
    /// - [`EngineError::Detail`] for empty picks.
    fn tree(&self, lua: &mlua::Lua, args: (Value, Value)) -> mlua::Result<mlua::Table> {
        const CALLER: &str = "ResourceHandle:tree";
        let scope = Scope::method(CALLER);
        let (dest, callback) = parse_tree_args(args, &scope)?;
        let sealed = seal(&self.handle, &self.stores)?;
        tree_from_archive(lua, &sealed, &self.stores, dest, callback, &scope)
    }

    /// Picks one archive member by its archive-relative name.
    ///
    /// # Errors
    ///
    /// - [`EngineError::Archive`] for non-archives and
    ///   unknown members.
    fn extract_member(&self, name: String) -> mlua::Result<LuaResourceHandle> {
        let sealed = seal(&self.handle, &self.stores)?;
        let member = self
            .stores
            .archives()
            .extract_member(&sealed, &name)
            .map_err(EngineError::from)?;
        Ok(LuaResourceHandle::new(
            member,
            self.stores.clone(),
            Some(sealed),
        ))
    }

    /// One-line identity for logs and errors.
    #[lua(meta, infallible)]
    fn __tostring(&self) -> String {
        format!(
            "ResourceHandle(sha256:{}, path:{})",
            self.handle.sha(),
            self.handle.canonical().display()
        )
    }
}

#[typeduserdata_impl]
impl LuaArchiveHandle {
    /// Lists member names without reading content.
    ///
    /// # Errors
    ///
    /// - [`EngineError::Archive`] for unreadable archives.
    fn members(&self) -> mlua::Result<Vec<String>> {
        Ok(self
            .stores
            .archives()
            .members(&self.handle)
            .map_err(EngineError::from)?)
    }

    /// Builds one tree document from the archive.
    ///
    /// The callback receives one resource handle per member
    /// and returns nil, true, or a `{ path, mode }` table.
    ///
    /// # Errors
    ///
    /// - [`EngineError::Field`] for misshaped arguments.
    /// - [`EngineError::Repeat`] for duplicate keeps.
    /// - [`EngineError::Detail`] for empty picks.
    fn tree(&self, lua: &mlua::Lua, args: (Value, Value)) -> mlua::Result<mlua::Table> {
        const CALLER: &str = "ArchiveHandle:tree";
        let scope = Scope::method(CALLER);
        let (dest, callback) = parse_tree_args(args, &scope)?;
        tree_from_archive(lua, &self.handle, &self.stores, dest, callback, &scope)
    }

    /// Picks one archive member by its archive-relative name.
    ///
    /// # Errors
    ///
    /// - [`EngineError::Archive`] for unknown members.
    fn extract_member(&self, name: String) -> mlua::Result<LuaResourceHandle> {
        let member = self
            .stores
            .archives()
            .extract_member(&self.handle, &name)
            .map_err(EngineError::from)?;
        Ok(LuaResourceHandle::new(
            member,
            self.stores.clone(),
            Some(self.handle.clone()),
        ))
    }

    /// One-line identity for logs and errors.
    #[lua(meta, infallible)]
    fn __tostring(&self) -> String {
        format!(
            "ArchiveHandle(sha256:{}, source:{})",
            self.handle.sha(),
            self.handle.canonical().display()
        )
    }
}

#[typeduserdata_impl]
impl LuaRoute {
    /// Destination base name.
    #[lua(infallible)]
    fn base(&self) -> String {
        self.handle.base().name().to_string()
    }

    /// Destination-relative path.
    #[lua(infallible)]
    fn relative(&self) -> String {
        self.handle.relative().to_string_lossy().into_owned()
    }

    /// Portable route display.
    #[lua(infallible)]
    fn display(&self) -> String {
        self.handle.display()
    }

    /// One-line identity for logs and errors.
    #[lua(meta, infallible)]
    fn __tostring(&self) -> String {
        format!(
            "Route(base:{:?}, path:{})",
            self.handle.base(),
            self.handle.relative().display()
        )
    }
}

/// Parses tree destination and callback args.
///
/// # Errors
///
/// - [`EngineError::Field`] for non-route destinations
///   and non-function callbacks.
fn parse_tree_args(args: (Value, Value), scope: &Scope) -> mlua::Result<(Route, mlua::Function)> {
    let (dest_value, callback_value) = args;
    let destination = req_route(&dest_value, scope, "dest")?;
    let Some(callback) = callback_value.as_function() else {
        return Err(EngineError::Field {
            scope: scope.clone(),
            field: FieldRef::name("callback"),
            want: "must be a function",
        }
        .into());
    };
    Ok((destination, callback.clone()))
}

/// Reads one destination route from a Lua value.
///
/// # Errors
///
/// - [`EngineError::Field`] for non-route values.
pub(crate) fn req_route(value: &Value, scope: &Scope, field: &str) -> mlua::Result<Route> {
    let field = FieldRef::name(field);
    let Some(data) = value.as_userdata() else {
        return Err(EngineError::Field {
            scope: scope.clone(),
            field,
            want: "must be a confit.path value",
        }
        .into());
    };
    match data.borrow::<LuaRoute>() {
        Ok(route) => Ok(route.core().clone()),
        Err(_) => Err(EngineError::Field {
            scope: scope.clone(),
            field,
            want: "must be a confit.path value",
        }
        .into()),
    }
}

/// Reads one patch target display from a string or a route.
///
/// Plain strings pass through intact for rc targets.
/// Route values translate to their portable display, so
/// patch targets name the same text the plan keys carry.
///
/// # Errors
///
/// - [`EngineError::Field`] for non-string non-route values.
pub(crate) fn req_target_path(value: &Value, scope: &Scope, field: &str) -> mlua::Result<String> {
    if let Some(text) = value.clone().opt_str() {
        return Ok(text);
    }
    if let Some(data) = value.as_userdata()
        && let Ok(route) = data.borrow::<LuaRoute>()
    {
        return Ok(route.core().display());
    }
    Err(EngineError::Field {
        scope: scope.clone(),
        field: FieldRef::name(field),
        want: "must be a string or a confit.path value",
    }
    .into())
}

/// Installs the fetch constructor on a session.
pub(crate) fn install(session: &crate::eval::Session) -> mlua::Result<()> {
    let lua = &session.lua;
    let confit = confit_table(lua)?;
    let stores = session.stores.clone();
    let root = session.root.clone();
    let re_fetch = session.re_fetch;
    confit.set(
        "fetch",
        lua.create_function(move |lua, args: (Value, Option<Value>)| {
            fetch_impl(lua, &stores, &root, re_fetch, args)
        })?,
    )?;
    Ok(())
}

/// Fetches one URL or project file into a handle.
///
/// URLs hold `://` and return fetch userdata. Anything
/// else reads exec-root-relative and returns resource
/// userdata.
///
/// # Errors
///
/// - [`EngineError::Field`] for empty inputs, url-only
///   options, and misshaped digests.
/// - [`EngineError::OptUnknown`] for unknown option fields.
/// - [`EngineError::ShaHolds`] for bad digests.
/// - [`EngineError::Fetch`] for transport failures.
/// - [`EngineError::Resource`] for missing project files.
fn fetch_impl(
    lua: &mlua::Lua,
    stores: &Stores,
    root: &std::path::Path,
    re_fetch: bool,
    args: (Value, Option<Value>),
) -> mlua::Result<Value> {
    const CALLER: &str = "confit.fetch";
    let scope = Scope::method(CALLER);
    let (input, opts) = args;
    let raw = input.req_str(&scope, "input")?;
    if raw.is_empty() {
        return Err(EngineError::Field {
            scope: scope.clone(),
            field: FieldRef::name("input"),
            want: "must not be empty",
        }
        .into());
    }
    if raw.contains("://") {
        let (url, wanted) = parse_fetch_args(raw, opts, &scope)?;
        return match stores.fetch().fetch(&url, wanted, re_fetch) {
            Ok(handle) => lua
                .create_userdata(LuaFetchHandle::new(handle, stores.clone()))
                .map(Value::UserData),
            Err(error) => Err(EngineError::from(error).into()),
        };
    }
    if opts.is_some_and(|opts| !opts.is_nil()) {
        return Err(EngineError::Field {
            scope: scope.clone(),
            field: FieldRef::name("opts"),
            want: "fits urls alone",
        }
        .into());
    }
    let rel = std::path::Path::new(&raw);
    let full = if rel.is_absolute() {
        rel.to_path_buf()
    } else {
        root.join(rel)
    };
    match stores.resources().resource(root, &full) {
        Ok(handle) => lua
            .create_userdata(LuaResourceHandle::new(handle, stores.clone(), None))
            .map(Value::UserData),
        Err(error) => Err(EngineError::from(error).into()),
    }
}

/// Parses URL and optional sha table for fetch calls.
///
/// # Errors
///
/// - [`EngineError::Field`] for misshaped options.
/// - [`EngineError::OptUnknown`] for unknown fields.
/// - [`EngineError::ShaHolds`] for bad digests.
fn parse_fetch_args(
    url: String,
    opts: Option<Value>,
    scope: &Scope,
) -> mlua::Result<(String, Option<Sha>)> {
    let Some(opts_value) = opts else {
        return Ok((url, None));
    };
    if opts_value.is_nil() {
        return Ok((url, None));
    }
    let table = opts_value.req_table(scope, "opts")?;
    let mut wanted: Option<Sha> = None;
    for pair in table.pairs::<Value, Value>() {
        let (key, value) = pair?;
        let Some(name) = key.opt_str() else {
            return Err(EngineError::Field {
                scope: scope.clone(),
                field: FieldRef::name("opts"),
                want: "must hold string keys",
            }
            .into());
        };
        if name != "sha256" {
            return Err(EngineError::OptUnknown {
                scope: scope.clone(),
                field: FieldRef::name("opts"),
                name,
            }
            .into());
        }
        let digest = value.req_str(scope, "sha256")?;
        let sha = Sha::new(digest).map_err(|parsed| EngineError::ShaHolds {
            scope: scope.clone(),
            reason: parsed.to_string(),
        })?;
        wanted = Some(sha);
    }
    Ok((url, wanted))
}
