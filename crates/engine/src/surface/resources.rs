//! Resources
//!
//! Root-relative file loads plus cached remote reads.

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use mlua::{Lua, Value};
use serde_json::Value as Json;

use super::confit_table;
use crate::error::plan_error;
use crate::fetch::Fetch;
use crate::lua::{JsonExt, ValueExt};
use confit_core::progress::{Event, ProgressSender};

/// Shared fetch inputs for one evaluation.
#[derive(Clone)]
struct FetchState {
    /// Project root for resource reads.
    root: PathBuf,
    /// Cache folder for remote bytes.
    cache: PathBuf,
    /// Forces downloads past the sidecar cache.
    re_fetch: bool,
    /// Network source behind the trait.
    fetcher: Arc<dyn Fetch>,
    /// Progress sender for fetch facts.
    progress: Option<ProgressSender>,
}

/// Installs the resources namespace on a state.
pub(crate) fn install(session: &crate::eval::Session) -> mlua::Result<()> {
    let lua = &session.lua;
    let state = FetchState {
        root: session.root.clone(),
        cache: session.cache.clone(),
        re_fetch: session.re_fetch,
        fetcher: session.fetcher.clone(),
        progress: session.progress.clone(),
    };
    let confit = confit_table(lua)?;
    let resources = lua.create_table()?;
    for name in ["load_toml", "load_json", "load_yaml", "load_text"] {
        let loader_state = state.clone();
        let loader =
            lua.create_function(move |lua, path: Value| loader_state.load_impl(lua, name, path))?;
        resources.set(name, loader)?;
    }
    let bytes_state = state.clone();
    let bytes_loader =
        lua.create_function(move |lua, path: Value| bytes_state.load_bytes_impl(lua, path))?;
    resources.set("load_bytes", bytes_loader)?;
    let text_state = state.clone();
    let text_loader = lua.create_function(move |lua, args: (Value, Option<Value>)| {
        text_state.fetch_text_impl(lua, args)
    })?;
    resources.set("fetch_text", text_loader)?;
    let file_state = state.clone();
    let file_loader = lua.create_function(move |lua, args: (Value, Option<Value>)| {
        file_state.fetch_file_impl(lua, args)
    })?;
    resources.set("fetch_file", file_loader)?;
    confit.set("resources", resources)?;
    Ok(())
}

impl FetchState {
    /// Loads one root-relative file through the matching decoder.
    fn load_impl(&self, lua: &Lua, name: &'static str, path: Value) -> mlua::Result<Value> {
        let caller = match name {
            "load_toml" => "confit.resources.load_toml",
            "load_json" => "confit.resources.load_json",
            "load_yaml" => "confit.resources.load_yaml",
            _ => "confit.resources.load_text",
        };
        let rel = path.req_str(caller, "path")?;
        self.load_decoded(lua, name, caller, rel)
    }

    /// Loads one root-relative file through the matching decoder.
    ///
    /// # Arguments
    ///
    /// * `lua` - state owning the output value.
    /// * `name` - loader name selecting the decoder.
    /// * `caller` - error prefix naming the constructor.
    /// * `rel` - project-relative path under reading.
    ///
    /// # Returns
    ///
    /// Decoded Lua value or the raw text string.
    ///
    /// # Errors
    ///
    /// Jail escapes fail as plan errors. Unreadable files fail as plan errors.
    /// Parse failures fail as plan errors.
    ///
    fn load_decoded(
        &self,
        lua: &Lua,
        name: &str,
        caller: &str,
        rel: String,
    ) -> mlua::Result<Value> {
        let full = resolve_under_root(&self.root, &self.cache, &rel, caller)?;
        let text = std::fs::read_to_string(&full)
            .map_err(|error| plan_error(format!("{caller}: cannot read '{rel}': {error}")))?;
        if name == "load_text" {
            return Ok(Value::String(lua.create_string(&text)?));
        }
        let json = match name {
            "load_toml" => {
                let parsed: toml::Value = toml::from_str(&text).map_err(|error| {
                    plan_error(format!("{caller}: cannot parse '{rel}': {error}"))
                })?;
                serde_json::to_value(&parsed).map_err(|error| {
                    plan_error(format!("{caller}: cannot convert '{rel}': {error}"))
                })?
            }
            "load_json" => serde_json::from_str::<Json>(&text)
                .map_err(|error| plan_error(format!("{caller}: cannot parse '{rel}': {error}")))?,
            _ => noyalib::from_str::<Json>(&text)
                .map_err(|error| plan_error(format!("{caller}: cannot parse '{rel}': {error}")))?,
        };
        json.to_lua(lua, caller)
    }

    /// Loads one root-relative file as raw bytes for opaque use.
    fn load_bytes_impl(&self, lua: &Lua, path: Value) -> mlua::Result<Value> {
        const CALLER: &str = "confit.resources.load_bytes";
        let rel = path.req_str(CALLER, "path")?;
        self.load_bytes_path(lua, CALLER, rel)
    }

    /// Loads one root-relative file as raw bytes for opaque use.
    ///
    /// # Arguments
    ///
    /// * `lua` - state owning the output string.
    /// * `caller` - error prefix naming the constructor.
    /// * `rel` - project-relative path under reading.
    ///
    /// # Returns
    ///
    /// Lua string holding the raw file bytes.
    ///
    /// # Errors
    ///
    /// Jail escapes fail as plan errors. Unreadable files fail as plan errors.
    ///
    fn load_bytes_path(&self, lua: &Lua, caller: &str, rel: String) -> mlua::Result<Value> {
        let full = resolve_under_root(&self.root, &self.cache, &rel, caller)?;
        let bytes = std::fs::read(&full)
            .map_err(|error| plan_error(format!("{caller}: cannot read '{rel}': {error}")))?;
        Ok(Value::String(lua.create_string(&bytes)?))
    }

    /// Fetches one URL body as a Lua string.
    fn fetch_text_impl(&self, lua: &Lua, args: (Value, Option<Value>)) -> mlua::Result<Value> {
        const CALLER: &str = "confit.resources.fetch_text";
        let (url, wanted) = Self::parse_fetch_args(args, CALLER)?;
        self.fetch_text_owned(lua, CALLER, url, wanted)
    }

    /// Fetches one URL body as a Lua string.
    ///
    /// # Arguments
    ///
    /// * `lua` - state owning the output string.
    /// * `caller` - error prefix naming the constructor.
    /// * `url` - URL under fetching.
    /// * `wanted` - expected sha256 holding `None` for no check.
    ///
    /// # Returns
    ///
    /// Lua string holding the response body.
    ///
    /// # Errors
    ///
    /// Fetch failures fail as plan errors. Sha mismatches fail as plan errors.
    /// Non-utf8 bodies fail as plan errors.
    ///
    fn fetch_text_owned(
        &self,
        lua: &Lua,
        caller: &str,
        url: String,
        wanted: Option<String>,
    ) -> mlua::Result<Value> {
        let bytes = self.fetch_bytes(caller, &url)?;
        check_user_sha(caller, &url, &bytes, wanted.as_deref())?;
        let text = String::from_utf8(bytes).map_err(|error| {
            plan_error(format!(
                "{caller}: body for '{url}' holds invalid utf8: {error}"
            ))
        })?;
        Ok(Value::String(lua.create_string(&text)?))
    }

    /// Fetches one URL into the cache and returns its path.
    fn fetch_file_impl(&self, lua: &Lua, args: (Value, Option<Value>)) -> mlua::Result<Value> {
        const CALLER: &str = "confit.resources.fetch_file";
        let (url, wanted) = Self::parse_fetch_args(args, CALLER)?;
        self.fetch_file_owned(lua, CALLER, url, wanted)
    }

    /// Fetches one URL into the cache and returns its path.
    ///
    /// # Arguments
    ///
    /// * `lua` - state owning the output string.
    /// * `caller` - error prefix naming the constructor.
    /// * `url` - URL under fetching.
    /// * `wanted` - expected sha256 holding `None` for no check.
    ///
    /// # Returns
    ///
    /// Lua string holding the cache file path.
    ///
    /// # Errors
    ///
    /// Fetch failures fail as plan errors. Sha mismatches fail as plan errors.
    ///
    fn fetch_file_owned(
        &self,
        lua: &Lua,
        caller: &str,
        url: String,
        wanted: Option<String>,
    ) -> mlua::Result<Value> {
        log::debug!("fetch start url={url}");
        if let Some(sender) = self.progress.as_ref() {
            let _ = sender.send(Event::FetchStarted { url: url.clone() });
        }
        let cache = crate::fetch::Cache::new(self.cache.clone());
        let start = std::time::Instant::now();
        if !self.re_fetch
            && let Some(stored) = cache.lookup(&url)
        {
            log::debug!(
                "fetch cache hit url={url} bytes={} took {}ms",
                stored.len(),
                start.elapsed().as_millis()
            );
            if let Some(sender) = self.progress.as_ref() {
                let _ = sender.send(Event::FetchCached {
                    url: url.clone(),
                    bytes: stored.len(),
                });
            }
            check_user_sha(caller, &url, &stored, wanted.as_deref())?;
            let path = self.cache_path(&url);
            return Ok(Value::String(
                lua.create_string(path.to_string_lossy().as_ref())?,
            ));
        }
        let download_start = std::time::Instant::now();
        let reader = match self.fetcher.fetch_stream(&url) {
            Ok(reader) => reader,
            Err(confit_core::error::Error::Plan(message)) => {
                return Err(plan_error(format!("{caller}: {message}")));
            }
            Err(confit_core::error::Error::Io(error)) => {
                return Err(plan_error(format!(
                    "{caller}: fetch '{url}' failed: {error}"
                )));
            }
        };
        let path = match cache.store_stream(&url, reader) {
            Ok(path) => path,
            Err(error) => {
                return Err(plan_error(format!(
                    "{caller}: cannot write cache for '{url}': {error}"
                )));
            }
        };
        let bytes_len = if wanted.is_some() {
            match std::fs::read(&path) {
                Ok(bytes) => {
                    check_user_sha(caller, &url, &bytes, wanted.as_deref())?;
                    bytes.len()
                }
                Err(error) => {
                    return Err(plan_error(format!(
                        "{caller}: cannot read cache for '{url}': {error}"
                    )));
                }
            }
        } else {
            match std::fs::metadata(&path) {
                Ok(facts) => usize::try_from(facts.len()).unwrap_or(0),
                Err(error) => {
                    return Err(plan_error(format!(
                        "{caller}: cannot read cache for '{url}': {error}"
                    )));
                }
            }
        };
        log::debug!(
            "fetch download url={url} bytes={bytes_len} took {}ms",
            download_start.elapsed().as_millis()
        );
        if let Some(sender) = self.progress.as_ref() {
            let _ = sender.send(Event::FetchDownloaded {
                url: url.clone(),
                bytes: bytes_len,
            });
        }
        Ok(Value::String(
            lua.create_string(path.to_string_lossy().as_ref())?,
        ))
    }

    /// Parses URL plus optional sha table for fetch calls.
    ///
    /// # Arguments
    ///
    /// * `args` - url plus optional opts table.
    /// * `caller` - error prefix naming the constructor.
    ///
    /// # Returns
    ///
    /// URL plus expected sha256 holding `None` for no check.
    ///
    /// # Errors
    ///
    /// Empty urls fail as plan errors. Non-table opts fail as plan errors.
    /// Unknown opts fields fail as plan errors. Bad digests fail as plan errors.
    ///
    fn parse_fetch_args(
        args: (Value, Option<Value>),
        caller: &str,
    ) -> mlua::Result<(String, Option<String>)> {
        let (url_value, opts_value) = args;
        let url = url_value.req_str(caller, "url")?;
        if url.is_empty() {
            return Err(plan_error(format!(
                "{caller}: field 'url' must not be empty"
            )));
        }
        let Some(opts_value) = opts_value else {
            return Ok((url, None));
        };
        if opts_value.is_nil() {
            return Ok((url, None));
        }
        let table = opts_value.req_table(caller, "opts")?;
        let mut wanted: Option<String> = None;
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
            if digest.len() != 64 || !digest.chars().all(|item| item.is_ascii_hexdigit()) {
                return Err(plan_error(format!(
                    "{caller}: field 'sha256' must be 64 hex chars"
                )));
            }
            wanted = Some(digest.to_lowercase());
        }
        Ok((url, wanted))
    }

    /// Returns cached bytes or downloads plus refreshes the sidecar.
    fn fetch_bytes(&self, caller: &str, url: &str) -> mlua::Result<Vec<u8>> {
        log::debug!("fetch start url={url}");
        if let Some(sender) = self.progress.as_ref() {
            let _ = sender.send(Event::FetchStarted {
                url: url.to_string(),
            });
        }
        let cache = crate::fetch::Cache::new(self.cache.clone());
        let start = std::time::Instant::now();
        if !self.re_fetch
            && let Some(stored) = cache.lookup(url)
        {
            log::debug!(
                "fetch cache hit url={url} bytes={} took {}ms",
                stored.len(),
                start.elapsed().as_millis()
            );
            if let Some(sender) = self.progress.as_ref() {
                let _ = sender.send(Event::FetchCached {
                    url: url.to_string(),
                    bytes: stored.len(),
                });
            }
            return Ok(stored);
        }
        let download_start = std::time::Instant::now();
        let bytes = match self.fetcher.fetch(url) {
            Ok(bytes) => bytes,
            Err(confit_core::error::Error::Plan(message)) => {
                return Err(plan_error(format!("{caller}: {message}")));
            }
            Err(confit_core::error::Error::Io(error)) => {
                return Err(plan_error(format!(
                    "{caller}: fetch '{url}' failed: {error}"
                )));
            }
        };
        if let Err(error) = cache.store(url, &bytes) {
            return Err(plan_error(format!(
                "{caller}: cannot write cache for '{url}': {error}"
            )));
        }
        log::debug!(
            "fetch download url={url} bytes={} took {}ms",
            bytes.len(),
            download_start.elapsed().as_millis()
        );
        if let Some(sender) = self.progress.as_ref() {
            let _ = sender.send(Event::FetchDownloaded {
                url: url.to_string(),
                bytes: bytes.len(),
            });
        }
        Ok(bytes)
    }

    /// Derives the cache file path for one URL.
    fn cache_path(&self, url: &str) -> PathBuf {
        crate::fetch::cache_path(&self.cache, url)
    }
}

/// Checks user sha against local or remote bytes alike.
fn check_user_sha(caller: &str, url: &str, bytes: &[u8], wanted: Option<&str>) -> mlua::Result<()> {
    let Some(expected) = wanted else {
        return Ok(());
    };
    let actual = confit_core::ids::sha256_hex(bytes);
    if actual != expected.to_lowercase() {
        return Err(plan_error(format!(
            "{caller}: sha256 mismatch for '{url}': want {expected}, got {actual}"
        )));
    }
    Ok(())
}

/// Resolves project-relative plus cache-absolute reads.
pub(crate) fn resolve_under_root(
    root: &Path,
    cache: &Path,
    rel: &str,
    caller: &str,
) -> mlua::Result<PathBuf> {
    if rel.is_empty() {
        return Err(plan_error(format!("{caller}: path must not be empty")));
    }
    let rel_path = Path::new(rel);
    if rel_path.is_absolute() {
        let candidate = PathBuf::from(rel);
        if under_cache(cache, &candidate) {
            return Ok(candidate);
        }
        return Err(plan_error(format!(
            "{caller}: path '{rel}' must be project-root-relative, not absolute"
        )));
    }
    let mut stack: Vec<String> = Vec::new();
    for component in rel_path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => {
                return Err(plan_error(format!(
                    "{caller}: path '{rel}' must be project-root-relative, not absolute"
                )));
            }
            Component::CurDir => {}
            Component::ParentDir => {
                if stack.pop().is_none() {
                    return Err(plan_error(format!(
                        "{caller}: path '{rel}' escapes the project root"
                    )));
                }
            }
            Component::Normal(segment) => stack.push(segment.to_string_lossy().into_owned()),
        }
    }
    let mut full = root.to_path_buf();
    for segment in &stack {
        full.push(segment);
    }
    if let (Ok(canonical_root), Ok(canonical_full)) = (root.canonicalize(), full.canonicalize())
        && !canonical_full.starts_with(&canonical_root)
    {
        return Err(plan_error(format!(
            "{caller}: path '{rel}' escapes the project root"
        )));
    }
    Ok(full)
}

/// Reports true while a path sits under the cache folder.
fn under_cache(cache: &Path, candidate: &Path) -> bool {
    if candidate.starts_with(cache) {
        if let (Ok(canonical_cache), Ok(canonical_candidate)) =
            (cache.canonicalize(), candidate.canonicalize())
            && !canonical_candidate.starts_with(&canonical_cache)
        {
            return false;
        }
        return true;
    }
    if let (Ok(canonical_cache), Ok(canonical_candidate)) =
        (cache.canonicalize(), candidate.canonicalize())
        && canonical_candidate.starts_with(&canonical_cache)
    {
        return true;
    }
    false
}
