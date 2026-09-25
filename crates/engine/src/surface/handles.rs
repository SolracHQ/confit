//! Handles
//!
//! Typed Lua handles over core identity plus the fetch constructor.

use std::io::Read as _;

use mlua::Value;
use mlua_extras::{TypedUserData, typeduserdata_impl};
use serde_json::Value as Json;

use super::confit_table;
use super::document::{check_rel, tree_table};
use crate::error::plan_error;
use crate::lua::{JsonExt, ValueExt};
use crate::model::TreeMemberDecl;
use confit_model::error::Error;
use confit_model::handles::{
    ArchiveHandle, BlobHandle, FetchHandle, ResourceHandle, Route, Sha, TrustedHandle,
};
use confit_model::progress::ProgressSender;
use confit_store::Stores;

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
/// plus the stored hash travel together from the blob pool.
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
/// Non-archives fail as plan errors naming the source.
fn seal(source: &dyn TrustedHandle, stores: &Stores, caller: &str) -> mlua::Result<ArchiveHandle> {
    match stores.archives().archive(source) {
        Ok(handle) => Ok(handle),
        Err(Error::Plan(message)) => Err(plan_error(format!("{caller}: {message}"))),
        Err(Error::Io(error)) => Err(plan_error(format!("{caller}: {error}"))),
    }
}

/// Reads member bytes from their archive spill.
///
/// # Errors
///
/// Unknown members and stream failures fail as plan errors.
fn member_bytes(member: &ResourceHandle, stores: &Stores, caller: &str) -> mlua::Result<Vec<u8>> {
    let archives = stores.archives();
    let mut reader = match archives.open_decompressed(member) {
        Ok(reader) => reader,
        Err(Error::Plan(message)) => return Err(plan_error(format!("{caller}: {message}"))),
        Err(Error::Io(error)) => return Err(plan_error(format!("{caller}: {error}"))),
    };
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).map_err(|error| {
        plan_error(format!(
            "{caller}: cannot read '{}': {error}",
            member.canonical().display()
        ))
    })?;
    Ok(bytes)
}

/// Opens member bytes from their archive spill.
///
/// # Errors
///
/// Unknown members fail as plan errors.
fn member_reader(
    member: &ResourceHandle,
    stores: &Stores,
    caller: &str,
) -> mlua::Result<Box<dyn std::io::Read>> {
    match stores.archives().open_decompressed(member) {
        Ok(reader) => Ok(reader),
        Err(Error::Plan(message)) => Err(plan_error(format!("{caller}: {message}"))),
        Err(Error::Io(error)) => Err(plan_error(format!("{caller}: {error}"))),
    }
}

/// Pools streamed bytes under content identity.
///
/// # Errors
///
/// Pool write failures fail as plan errors.
fn pool_reader(
    stores: &Stores,
    reader: &mut dyn std::io::Read,
    caller: &str,
) -> mlua::Result<(BlobHandle, u64)> {
    let blobs = stores.blobs();
    let handle = match blobs.put_reader(reader) {
        Ok(handle) => handle,
        Err(Error::Plan(message)) => return Err(plan_error(format!("{caller}: {message}"))),
        Err(Error::Io(error)) => return Err(plan_error(format!("{caller}: {error}"))),
    };
    let size = match blobs.len(&handle) {
        Ok(size) => size,
        Err(Error::Plan(message)) => return Err(plan_error(format!("{caller}: {message}"))),
        Err(Error::Io(error)) => return Err(plan_error(format!("{caller}: {error}"))),
    };
    Ok((handle, size))
}

/// Pools trusted source bytes under content identity.
///
/// # Errors
///
/// Pool write failures fail as plan errors.
fn pool_source(
    stores: &Stores,
    source: &dyn TrustedHandle,
    caller: &str,
) -> mlua::Result<(BlobHandle, u64)> {
    let blobs = stores.blobs();
    let handle = match blobs.put_source(source) {
        Ok(handle) => handle,
        Err(Error::Plan(message)) => return Err(plan_error(format!("{caller}: {message}"))),
        Err(Error::Io(error)) => return Err(plan_error(format!("{caller}: {error}"))),
    };
    let size = match blobs.len(&handle) {
        Ok(size) => size,
        Err(Error::Plan(message)) => return Err(plan_error(format!("{caller}: {message}"))),
        Err(Error::Io(error)) => return Err(plan_error(format!("{caller}: {error}"))),
    };
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
/// Unreadable cache files fail as plan errors.
fn fetch_bytes(handle: &LuaFetchHandle, caller: &str) -> mlua::Result<Vec<u8>> {
    match handle.stores.fetch().read(&handle.handle) {
        Ok(bytes) => Ok(bytes),
        Err(Error::Plan(message)) => Err(plan_error(format!("{caller}: {message}"))),
        Err(Error::Io(error)) => Err(plan_error(format!("{caller}: {error}"))),
    }
}

/// Reads resource bytes from the workspace or the archive spill.
///
/// # Errors
///
/// Unreadable files fail as plan errors.
fn resource_bytes(handle: &LuaResourceHandle, caller: &str) -> mlua::Result<Vec<u8>> {
    if handle.archive.is_some() {
        return member_bytes(&handle.handle, &handle.stores, caller);
    }
    match handle.stores.resources().read_text(&handle.handle) {
        Ok(text) => Ok(text.into_bytes()),
        Err(Error::Plan(message)) => Err(plan_error(format!("{caller}: {message}"))),
        Err(Error::Io(error)) => Err(plan_error(format!("{caller}: {error}"))),
    }
}

/// Decodes handle bytes into a Lua table.
///
/// # Errors
///
/// Malformed documents fail as plan errors. Non-table
/// documents fail as plan errors.
fn decode_bytes(
    lua: &mlua::Lua,
    bytes: &[u8],
    format: DecodeFormat,
    caller: &str,
) -> mlua::Result<mlua::Table> {
    let parsed: Json = match format {
        DecodeFormat::Toml => {
            let text = std::str::from_utf8(bytes)
                .map_err(|error| plan_error(format!("{caller}: {error}")))?;
            let value: toml::Value =
                toml::from_str(text).map_err(|error| plan_error(format!("{caller}: {error}")))?;
            serde_json::to_value(&value)
                .map_err(|error| plan_error(format!("{caller}: {error}")))?
        }
        DecodeFormat::Json => {
            let value: Json = serde_json::from_slice(bytes)
                .map_err(|error| plan_error(format!("{caller}: {error}")))?;
            serde_json::to_value(&value)
                .map_err(|error| plan_error(format!("{caller}: {error}")))?
        }
        DecodeFormat::Yaml => {
            let text = std::str::from_utf8(bytes)
                .map_err(|error| plan_error(format!("{caller}: {error}")))?;
            let value: noyalib::Value = noyalib::from_str(text)
                .map_err(|error| plan_error(format!("{caller}: {error}")))?;
            serde_json::to_value(&value)
                .map_err(|error| plan_error(format!("{caller}: {error}")))?
        }
    };
    match parsed.to_lua(lua, caller)? {
        Value::Table(table) => Ok(table),
        _ => Err(plan_error(format!("{caller}: document must hold a table"))),
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
/// Non-handle sources fail as plan errors. Pool write
/// failures fail as plan errors.
pub(crate) fn blob_for_opaque(
    value: &Value,
    stores: &Stores,
    ctor: &str,
) -> mlua::Result<(BlobHandle, u64)> {
    let Some(data) = value.as_userdata() else {
        return Err(plan_error(format!(
            "{ctor}: field 'src' must be a fetch, resource, or archive member handle"
        )));
    };
    if let Ok(handle) = data.borrow::<LuaFetchHandle>() {
        return pool_source(stores, &handle.handle, ctor);
    }
    if let Ok(handle) = data.borrow::<LuaResourceHandle>() {
        if handle.archive.is_some() {
            let mut reader = member_reader(&handle.handle, stores, ctor)?;
            return pool_reader(stores, &mut *reader, ctor);
        }
        return pool_source(stores, &handle.handle, ctor);
    }
    Err(plan_error(format!(
        "{ctor}: field 'src' must be a fetch, resource, or archive member handle"
    )))
}

/// Builds one tree document table from a sealed archive.
///
/// Members pass the callback one by one. Nil skips, true
/// keeps at the same path, a table overrides path plus
/// mode. Kept members pool their bytes once and land in
/// relative path order.
///
/// # Errors
///
/// Non-archives fail as plan errors. Bad callback returns,
/// duplicate keeps, and empty picks fail as plan errors.
fn tree_from_archive(
    lua: &mlua::Lua,
    sealed: &ArchiveHandle,
    stores: &Stores,
    destination: Route,
    callback: mlua::Function,
    caller: &str,
) -> mlua::Result<mlua::Table> {
    let archives = stores.archives();
    let names = match archives.members(sealed) {
        Ok(names) => names,
        Err(Error::Plan(message)) => return Err(plan_error(format!("{caller}: {message}"))),
        Err(Error::Io(error)) => return Err(plan_error(format!("{caller}: {error}"))),
    };
    let mut kept: Vec<TreeMemberDecl> = Vec::new();
    for name in &names {
        let member = match archives.extract_member(sealed, name) {
            Ok(member) => member,
            Err(Error::Plan(message)) => return Err(plan_error(format!("{caller}: {message}"))),
            Err(Error::Io(error)) => return Err(plan_error(format!("{caller}: {error}"))),
        };
        let lua_member =
            LuaResourceHandle::new(member.clone(), stores.clone(), Some(sealed.clone()));
        let returned: Value = callback.call(lua_member)?;
        let Some(pick) = parse_tree_pick(&returned, name, caller)? else {
            continue;
        };
        let mode = match pick.mode {
            Some(mode) => mode,
            None => match archives.mode(&member) {
                Ok(mode) => mode,
                Err(Error::Plan(message)) => {
                    return Err(plan_error(format!("{caller}: {message}")));
                }
                Err(Error::Io(error)) => return Err(plan_error(format!("{caller}: {error}"))),
            },
        };
        if kept.iter().any(|item| item.rel == pick.rel) {
            return Err(plan_error(format!(
                "{caller}: tree keeps '{}' more than once",
                pick.rel
            )));
        }
        let mut reader = member_reader(&member, stores, caller)?;
        let (blob, size) = pool_reader(stores, &mut *reader, caller)?;
        kept.push(TreeMemberDecl {
            rel: pick.rel,
            blob,
            size,
            mode,
        });
    }
    if kept.is_empty() {
        return Err(plan_error(format!(
            "{caller}: tree kept no members: check the pick filter"
        )));
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
/// overrides path plus mode. Anything else fails.
///
/// # Errors
///
/// Bad shapes, bad paths, and bad modes fail as plan errors.
fn parse_tree_pick(returned: &Value, name: &str, caller: &str) -> mlua::Result<Option<TreePick>> {
    if returned.is_nil() {
        return Ok(None);
    }
    if let Some(keep) = returned.as_boolean() {
        if keep {
            check_rel(caller, name)?;
            return Ok(Some(TreePick {
                rel: name.to_string(),
                mode: None,
            }));
        }
        return Err(plan_error(format!(
            "{caller}: callback must return nil, true, or a {{ path, mode }} table"
        )));
    }
    let Some(table) = returned.as_table() else {
        return Err(plan_error(format!(
            "{caller}: callback must return nil, true, or a {{ path, mode }} table"
        )));
    };
    for pair in table.pairs::<Value, Value>() {
        let (key, _) = pair?;
        let Some(field) = key.opt_str() else {
            return Err(plan_error(format!(
                "{caller}: callback table must hold string keys"
            )));
        };
        if field != "path" && field != "mode" {
            return Err(plan_error(format!(
                "{caller}: callback table unknown field '{field}'"
            )));
        }
    }
    let path_value: Value = table.get("path")?;
    let Some(rel) = path_value.opt_str() else {
        return Err(plan_error(format!(
            "{caller}: callback table field 'path' must be a string"
        )));
    };
    check_rel(caller, &rel)?;
    let mode_value: Value = table.get("mode")?;
    let mode = if mode_value.is_nil() {
        None
    } else if let Some(bits) = mode_value.as_integer() {
        if !(0..=0o7777).contains(&bits) {
            return Err(plan_error(format!(
                "{caller}: callback table field 'mode' must hold permission bits"
            )));
        }
        Some(bits as u32)
    } else if let Some(raw) = mode_value.opt_str() {
        Some(
            confit_model::document::parse_mode(&raw)
                .map_err(|error| plan_error(format!("{caller}: {error}")))?,
        )
    } else {
        return Err(plan_error(format!(
            "{caller}: callback table field 'mode' must be a string or integer"
        )));
    };
    Ok(Some(TreePick { rel, mode }))
}

#[typeduserdata_impl]
impl LuaFetchHandle {
    /// Decoded body text for the fetched artifact.
    ///
    /// # Errors
    ///
    /// Unreadable cache files fail as plan errors. Bodies
    /// outside UTF-8 fail as plan errors.
    fn text(&self) -> mlua::Result<String> {
        const CALLER: &str = "FetchHandle:text";
        let bytes = match self.stores.fetch().read(&self.handle) {
            Ok(bytes) => bytes,
            Err(Error::Plan(message)) => return Err(plan_error(format!("{CALLER}: {message}"))),
            Err(Error::Io(error)) => return Err(plan_error(format!("{CALLER}: {error}"))),
        };
        String::from_utf8(bytes).map_err(|error| {
            plan_error(format!(
                "{CALLER}: body for '{}' holds invalid utf8: {error}",
                self.handle.origin()
            ))
        })
    }

    /// Decoded TOML table for the fetched artifact.
    ///
    /// # Errors
    ///
    /// Unreadable cache files fail as plan errors. Malformed
    /// documents fail as plan errors.
    fn toml(&self, lua: &mlua::Lua) -> mlua::Result<mlua::Table> {
        const CALLER: &str = "FetchHandle:toml";
        let bytes = fetch_bytes(self, CALLER)?;
        decode_bytes(lua, &bytes, DecodeFormat::Toml, CALLER)
    }

    /// Decoded JSON table for the fetched artifact.
    ///
    /// # Errors
    ///
    /// Unreadable cache files fail as plan errors. Malformed
    /// documents fail as plan errors.
    fn json(&self, lua: &mlua::Lua) -> mlua::Result<mlua::Table> {
        const CALLER: &str = "FetchHandle:json";
        let bytes = fetch_bytes(self, CALLER)?;
        decode_bytes(lua, &bytes, DecodeFormat::Json, CALLER)
    }

    /// Decoded YAML table for the fetched artifact.
    ///
    /// # Errors
    ///
    /// Unreadable cache files fail as plan errors. Malformed
    /// documents fail as plan errors.
    fn yaml(&self, lua: &mlua::Lua) -> mlua::Result<mlua::Table> {
        const CALLER: &str = "FetchHandle:yaml";
        let bytes = fetch_bytes(self, CALLER)?;
        decode_bytes(lua, &bytes, DecodeFormat::Yaml, CALLER)
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
    /// Non-archives fail as plan errors naming the source.
    fn archive(&self) -> mlua::Result<LuaArchiveHandle> {
        const CALLER: &str = "FetchHandle:archive";
        seal(&self.handle, &self.stores, CALLER)
            .map(|handle| LuaArchiveHandle::new(handle, self.stores.clone()))
    }

    /// Builds one tree document from the fetched archive.
    ///
    /// The callback receives one resource handle per member
    /// and returns nil, true, or a `{ path, mode }` table.
    ///
    /// # Errors
    ///
    /// Non-archives fail as plan errors. Bad picks, duplicate
    /// keeps, and empty picks fail as plan errors.
    fn tree(&self, lua: &mlua::Lua, args: (Value, Value)) -> mlua::Result<mlua::Table> {
        const CALLER: &str = "FetchHandle:tree";
        let (dest, callback) = parse_tree_args(args, CALLER)?;
        let sealed = seal(&self.handle, &self.stores, CALLER)?;
        tree_from_archive(lua, &sealed, &self.stores, dest, callback, CALLER)
    }

    /// Picks one archive member by its archive-relative name.
    ///
    /// # Errors
    ///
    /// Non-archives and unknown members fail as plan errors.
    fn extract_member(&self, name: String) -> mlua::Result<LuaResourceHandle> {
        const CALLER: &str = "FetchHandle:extract_member";
        let sealed = seal(&self.handle, &self.stores, CALLER)?;
        match self.stores.archives().extract_member(&sealed, &name) {
            Ok(member) => Ok(LuaResourceHandle::new(
                member,
                self.stores.clone(),
                Some(sealed),
            )),
            Err(Error::Plan(message)) => Err(plan_error(format!("{CALLER}: {message}"))),
            Err(Error::Io(error)) => Err(plan_error(format!("{CALLER}: {error}"))),
        }
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
    /// Unreadable files fail as plan errors. Bodies outside
    /// UTF-8 fail as plan errors.
    fn text(&self) -> mlua::Result<String> {
        const CALLER: &str = "ResourceHandle:text";
        if self.archive.is_none() {
            return match self.stores.resources().read_text(&self.handle) {
                Ok(text) => Ok(text),
                Err(Error::Plan(message)) => Err(plan_error(format!("{CALLER}: {message}"))),
                Err(Error::Io(error)) => Err(plan_error(format!("{CALLER}: {error}"))),
            };
        }
        let bytes = member_bytes(&self.handle, &self.stores, CALLER)?;
        String::from_utf8(bytes).map_err(|error| {
            plan_error(format!(
                "{CALLER}: file '{}' holds invalid utf8: {error}",
                self.handle.canonical().display()
            ))
        })
    }

    /// Decoded TOML table for the project file or member.
    ///
    /// # Errors
    ///
    /// Unreadable files fail as plan errors. Malformed
    /// documents fail as plan errors.
    fn toml(&self, lua: &mlua::Lua) -> mlua::Result<mlua::Table> {
        const CALLER: &str = "ResourceHandle:toml";
        let bytes = resource_bytes(self, CALLER)?;
        decode_bytes(lua, &bytes, DecodeFormat::Toml, CALLER)
    }

    /// Decoded JSON table for the project file or member.
    ///
    /// # Errors
    ///
    /// Unreadable files fail as plan errors. Malformed
    /// documents fail as plan errors.
    fn json(&self, lua: &mlua::Lua) -> mlua::Result<mlua::Table> {
        const CALLER: &str = "ResourceHandle:json";
        let bytes = resource_bytes(self, CALLER)?;
        decode_bytes(lua, &bytes, DecodeFormat::Json, CALLER)
    }

    /// Decoded YAML table for the project file or member.
    ///
    /// # Errors
    ///
    /// Unreadable files fail as plan errors. Malformed
    /// documents fail as plan errors.
    fn yaml(&self, lua: &mlua::Lua) -> mlua::Result<mlua::Table> {
        const CALLER: &str = "ResourceHandle:yaml";
        let bytes = resource_bytes(self, CALLER)?;
        decode_bytes(lua, &bytes, DecodeFormat::Yaml, CALLER)
    }

    /// File name for the resource path.
    ///
    /// # Errors
    ///
    /// Paths without a file name fail as plan errors.
    fn name(&self) -> mlua::Result<String> {
        const CALLER: &str = "ResourceHandle:name";
        match self
            .handle
            .canonical()
            .file_name()
            .and_then(|name| name.to_str())
        {
            Some(name) => Ok(name.to_string()),
            None => Err(plan_error(format!(
                "{CALLER}: path '{}' holds no file name",
                self.handle.canonical().display()
            ))),
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
    /// Non-archives fail as plan errors naming the source.
    fn archive(&self) -> mlua::Result<LuaArchiveHandle> {
        const CALLER: &str = "ResourceHandle:archive";
        seal(&self.handle, &self.stores, CALLER)
            .map(|handle| LuaArchiveHandle::new(handle, self.stores.clone()))
    }

    /// Builds one tree document from the resource archive.
    ///
    /// The callback receives one resource handle per member
    /// and returns nil, true, or a `{ path, mode }` table.
    ///
    /// # Errors
    ///
    /// Non-archives fail as plan errors. Bad picks, duplicate
    /// keeps, and empty picks fail as plan errors.
    fn tree(&self, lua: &mlua::Lua, args: (Value, Value)) -> mlua::Result<mlua::Table> {
        const CALLER: &str = "ResourceHandle:tree";
        let (dest, callback) = parse_tree_args(args, CALLER)?;
        let sealed = seal(&self.handle, &self.stores, CALLER)?;
        tree_from_archive(lua, &sealed, &self.stores, dest, callback, CALLER)
    }

    /// Picks one archive member by its archive-relative name.
    ///
    /// # Errors
    ///
    /// Non-archives and unknown members fail as plan errors.
    fn extract_member(&self, name: String) -> mlua::Result<LuaResourceHandle> {
        const CALLER: &str = "ResourceHandle:extract_member";
        let sealed = seal(&self.handle, &self.stores, CALLER)?;
        match self.stores.archives().extract_member(&sealed, &name) {
            Ok(member) => Ok(LuaResourceHandle::new(
                member,
                self.stores.clone(),
                Some(sealed),
            )),
            Err(Error::Plan(message)) => Err(plan_error(format!("{CALLER}: {message}"))),
            Err(Error::Io(error)) => Err(plan_error(format!("{CALLER}: {error}"))),
        }
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
    /// Unreadable archives fail as plan errors.
    fn members(&self) -> mlua::Result<Vec<String>> {
        const CALLER: &str = "ArchiveHandle:members";
        match self.stores.archives().members(&self.handle) {
            Ok(names) => Ok(names),
            Err(Error::Plan(message)) => Err(plan_error(format!("{CALLER}: {message}"))),
            Err(Error::Io(error)) => Err(plan_error(format!("{CALLER}: {error}"))),
        }
    }

    /// Builds one tree document from the archive.
    ///
    /// The callback receives one resource handle per member
    /// and returns nil, true, or a `{ path, mode }` table.
    ///
    /// # Errors
    ///
    /// Bad picks, duplicate keeps, and empty picks fail as plan errors.
    fn tree(&self, lua: &mlua::Lua, args: (Value, Value)) -> mlua::Result<mlua::Table> {
        const CALLER: &str = "ArchiveHandle:tree";
        let (dest, callback) = parse_tree_args(args, CALLER)?;
        tree_from_archive(lua, &self.handle, &self.stores, dest, callback, CALLER)
    }

    /// Picks one archive member by its archive-relative name.
    ///
    /// # Errors
    ///
    /// Unknown members fail as plan errors naming the member.
    fn extract_member(&self, name: String) -> mlua::Result<LuaResourceHandle> {
        const CALLER: &str = "ArchiveHandle:extract_member";
        match self.stores.archives().extract_member(&self.handle, &name) {
            Ok(member) => Ok(LuaResourceHandle::new(
                member,
                self.stores.clone(),
                Some(self.handle.clone()),
            )),
            Err(Error::Plan(message)) => Err(plan_error(format!("{CALLER}: {message}"))),
            Err(Error::Io(error)) => Err(plan_error(format!("{CALLER}: {error}"))),
        }
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

/// Parses tree destination plus callback args.
///
/// # Errors
///
/// Non-route destinations and non-function callbacks fail
/// as plan errors.
fn parse_tree_args(args: (Value, Value), caller: &str) -> mlua::Result<(Route, mlua::Function)> {
    let (dest_value, callback_value) = args;
    let destination = req_route(&dest_value, caller, "dest")?;
    let Some(callback) = callback_value.as_function() else {
        return Err(plan_error(format!(
            "{caller}: field 'callback' must be a function"
        )));
    };
    Ok((destination, callback.clone()))
}

/// Reads one destination route from a Lua value.
///
/// # Errors
///
/// Non-route values fail as plan errors naming the field.
pub(crate) fn req_route(value: &Value, caller: &str, field: &str) -> mlua::Result<Route> {
    let Some(data) = value.as_userdata() else {
        return Err(plan_error(format!(
            "{caller}: field '{field}' must be a confit.path value"
        )));
    };
    match data.borrow::<LuaRoute>() {
        Ok(route) => Ok(route.core().clone()),
        Err(_) => Err(plan_error(format!(
            "{caller}: field '{field}' must be a confit.path value"
        ))),
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
/// Non-string non-route values fail as plan errors.
pub(crate) fn req_target_path(value: &Value, caller: &str, field: &str) -> mlua::Result<String> {
    if let Some(text) = value.clone().opt_str() {
        return Ok(text);
    }
    if let Some(data) = value.as_userdata()
        && let Ok(route) = data.borrow::<LuaRoute>()
    {
        return Ok(route.core().display());
    }
    Err(plan_error(format!(
        "{caller}: field '{field}' must be a string or a confit.path value"
    )))
}

/// Installs the fetch constructor on a session.
pub(crate) fn install(session: &crate::eval::Session) -> mlua::Result<()> {
    let lua = &session.lua;
    let confit = confit_table(lua)?;
    let stores = session.stores.clone();
    let root = session.root.clone();
    let re_fetch = session.re_fetch;
    let progress = session.progress.clone();
    confit.set(
        "fetch",
        lua.create_function(move |lua, args: (Value, Option<Value>)| {
            fetch_impl(lua, &stores, &root, re_fetch, progress.as_ref(), args)
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
/// Empty inputs fail as plan errors. Unknown opts fields
/// fail as plan errors. Bad digests fail as plan errors.
/// Transport, escape, and missing-file failures fail as
/// plan errors.
fn fetch_impl(
    lua: &mlua::Lua,
    stores: &Stores,
    root: &std::path::Path,
    re_fetch: bool,
    progress: Option<&ProgressSender>,
    args: (Value, Option<Value>),
) -> mlua::Result<Value> {
    const CALLER: &str = "confit.fetch";
    let (input, opts) = args;
    let raw = input.req_str(CALLER, "input")?;
    if raw.is_empty() {
        return Err(plan_error(format!(
            "{CALLER}: field 'input' must not be empty"
        )));
    }
    if raw.contains("://") {
        let (url, wanted) = parse_fetch_args(raw, opts, CALLER)?;
        return match stores.fetch().fetch(&url, wanted, re_fetch, progress) {
            Ok(handle) => lua
                .create_userdata(LuaFetchHandle::new(handle, stores.clone()))
                .map(Value::UserData),
            Err(Error::Plan(message)) => Err(plan_error(format!("{CALLER}: {message}"))),
            Err(Error::Io(error)) => Err(plan_error(format!(
                "{CALLER}: fetch '{url}' failed: {error}"
            ))),
        };
    }
    if opts.is_some_and(|opts| !opts.is_nil()) {
        return Err(plan_error(format!(
            "{CALLER}: field 'opts' fits urls alone"
        )));
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
        Err(Error::Plan(message)) => Err(plan_error(format!("{CALLER}: {message}"))),
        Err(Error::Io(error)) => Err(plan_error(format!("{CALLER}: {error}"))),
    }
}

/// Parses URL plus optional sha table for fetch calls.
///
/// # Arguments
///
/// * `url` - the raw url under fetching.
/// * `opts` - optional opts table value.
/// * `caller` - error prefix naming the constructor.
///
/// # Returns
///
/// URL and expected sha256 holding `None` for no check.
///
/// # Errors
///
/// Non-table opts fail as plan errors. Unknown opts fields
/// fail as plan errors. Bad digests fail as plan errors.
fn parse_fetch_args(
    url: String,
    opts: Option<Value>,
    caller: &str,
) -> mlua::Result<(String, Option<Sha>)> {
    let Some(opts_value) = opts else {
        return Ok((url, None));
    };
    if opts_value.is_nil() {
        return Ok((url, None));
    }
    let table = opts_value.req_table(caller, "opts")?;
    let mut wanted: Option<Sha> = None;
    for pair in table.pairs::<Value, Value>() {
        let (key, value) = pair?;
        let Some(name) = key.opt_str() else {
            return Err(plan_error(format!(
                "{caller}: field 'opts' must hold string keys"
            )));
        };
        if name != "sha256" {
            return Err(plan_error(format!(
                "{caller}: field 'opts' unknown field '{name}'"
            )));
        }
        let digest = value.req_str(caller, "sha256")?;
        let sha = Sha::new(digest)
            .map_err(|_| plan_error(format!("{caller}: field 'sha256' must be 64 hex chars")))?;
        wanted = Some(sha);
    }
    Ok((url, wanted))
}
