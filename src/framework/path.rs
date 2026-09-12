//! Path
//!
//! Home, config, data, and confroot path joins for Lua.

use std::path::{Path, PathBuf};

use mlua::{Lua, MultiValue, Table, Value};

use super::confit_table;
use crate::error::{Error, Result};

/// Installs the path namespace on a Lua state.
///
/// # Arguments
///
/// * `lua` - state receiving the namespace.
/// * `root` - project root for confroot joins.
///
/// # Errors
///
/// Fails with config errors for missing home directories and with Lua errors for table creation failures.
pub fn install(lua: &Lua, root: PathBuf) -> Result<()> {
    let base_dirs = directories::BaseDirs::new().ok_or_else(|| {
        Error::Config(
            "confit.path: cannot resolve home directory: BaseDirs::new returned None; set HOME to a valid directory".to_string(),
        )
    })?;
    let home_base = base_dirs.home_dir().to_path_buf();
    let config_base = base_dirs.config_dir().to_path_buf();
    let data_base = base_dirs.data_dir().to_path_buf();

    let path = lua
        .create_table()
        .map_err(|err| Error::Lua(err.to_string()))?;

    register_helper(lua, &path, "home", home_base)?;
    register_helper(lua, &path, "config", config_base)?;
    register_helper(lua, &path, "data", data_base)?;
    register_helper(lua, &path, "confroot", root)?;

    confit_table(lua)
        .map_err(|err| Error::Lua(err.to_string()))?
        .set("path", path)
        .map_err(|err| Error::Lua(err.to_string()))?;
    Ok(())
}

/// Registers one path helper joining segments under a base.
///
/// # Arguments
///
/// * `lua` - state owning the callback.
/// * `table` - namespace receiving the helper.
/// * `name` - helper name for calls plus errors.
/// * `base` - base folder for joins.
///
/// # Errors
///
/// Fails with Lua errors for callback creation plus registration failures.
fn register_helper(lua: &Lua, table: &Table, name: &'static str, base: PathBuf) -> Result<()> {
    let helper = lua
        .create_function(move |_, args: MultiValue| {
            let segments = take_segments(args, name)?;
            Ok::<String, mlua::Error>(join_under(&base, &segments))
        })
        .map_err(|err| Error::Lua(err.to_string()))?;
    table
        .set(name, helper)
        .map_err(|err| Error::Lua(err.to_string()))?;
    Ok(())
}

/// Joins segments under a base path.
///
/// # Arguments
///
/// * `base` - base folder.
/// * `segments` - tail segments.
///
/// # Returns
///
/// Joined path as a string.
fn join_under(base: &Path, segments: &[String]) -> String {
    let mut out = base.to_path_buf();
    for segment in segments {
        out.push(segment);
    }
    out.to_string_lossy().into_owned()
}

/// Collects variadic Lua arguments as strings.
///
/// # Arguments
///
/// * `args` - raw call arguments.
/// * `func` - helper name for errors.
///
/// # Returns
///
/// String segments in call order.
///
/// # Errors
///
/// Fails with Lua errors for segments holding values of other shapes, naming the index.
fn take_segments(args: MultiValue, func: &str) -> mlua::Result<Vec<String>> {
    let mut segments = Vec::with_capacity(args.len());
    for (position, value) in args.into_iter().enumerate() {
        let index = position + 1;
        match value {
            Value::String(text) => segments.push(text.to_string_lossy()),
            _ => {
                return Err(lua_err!(
                    "confit.path.{func}: segment [{index}] must be a string"
                ));
            }
        }
    }
    Ok(segments)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serialize tests that mutate process env vars.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Saved env values restored on drop, even on test panic.
    struct EnvRestore {
        home: Option<std::ffi::OsString>,
        config: Option<std::ffi::OsString>,
        data: Option<std::ffi::OsString>,
    }

    impl Drop for EnvRestore {
        fn drop(&mut self) {
            unsafe {
                match &self.home {
                    Some(value) => std::env::set_var("HOME", value),
                    None => std::env::remove_var("HOME"),
                }
                match &self.config {
                    Some(value) => std::env::set_var("XDG_CONFIG_HOME", value),
                    None => std::env::remove_var("XDG_CONFIG_HOME"),
                }
                match &self.data {
                    Some(value) => std::env::set_var("XDG_DATA_HOME", value),
                    None => std::env::remove_var("XDG_DATA_HOME"),
                }
            }
        }
    }

    /// Points process env vars at temp directories.
    ///
    /// # Arguments
    ///
    /// * `home` - override home directory.
    /// * `config` - override config directory.
    /// * `data` - override data directory.
    ///
    /// # Returns
    ///
    /// Env lock guard plus restore guard.
    fn pin_env(
        home: &Path,
        config: &Path,
        data: &Path,
    ) -> (std::sync::MutexGuard<'static, ()>, EnvRestore) {
        let guard = ENV_LOCK.lock().expect("env lock");
        let saved = EnvRestore {
            home: std::env::var_os("HOME"),
            config: std::env::var_os("XDG_CONFIG_HOME"),
            data: std::env::var_os("XDG_DATA_HOME"),
        };
        unsafe {
            std::env::set_var("HOME", home);
            std::env::set_var("XDG_CONFIG_HOME", config);
            std::env::set_var("XDG_DATA_HOME", data);
        }
        (guard, saved)
    }

    /// Builds a Lua state holding the path namespace.
    ///
    /// # Arguments
    ///
    /// * `root` - project root for confroot joins.
    ///
    /// # Returns
    ///
    /// Lua state holding the namespace.
    fn setup(root: &Path) -> Lua {
        let lua = Lua::new();
        install(&lua, root.to_path_buf()).expect("install");
        lua
    }

    #[test]
    fn join_under_appends_segments() {
        assert_eq!(
            join_under(Path::new("/home/u"), &["a".to_string(), "b".to_string()]),
            "/home/u/a/b"
        );
    }

    #[test]
    fn join_under_without_segments_returns_base() {
        assert_eq!(join_under(Path::new("/home/u"), &[]), "/home/u");
    }

    #[test]
    fn lua_helpers_join_controlled_env() {
        let home_dir = tempfile::tempdir().expect("home tempdir");
        let config_dir = home_dir.path().join("cfg");
        let data_dir = home_dir.path().join("share");
        std::fs::create_dir_all(&config_dir).expect("mkdir config");
        std::fs::create_dir_all(&data_dir).expect("mkdir data");
        let root_dir = tempfile::tempdir().expect("root tempdir");
        let (_lock, _restore) = pin_env(home_dir.path(), &config_dir, &data_dir);

        let lua = setup(root_dir.path());
        let home: String = lua
            .load(r#"return confit.path.home("src", "x")"#)
            .eval()
            .expect("home");
        assert_eq!(
            home,
            home_dir.path().join("src").join("x").to_string_lossy()
        );
        let config: String = lua
            .load(r#"return confit.path.config("starship.toml")"#)
            .eval()
            .expect("config");
        assert_eq!(config, config_dir.join("starship.toml").to_string_lossy());
        let data: String = lua
            .load(r#"return confit.path.data("fonts", "f")"#)
            .eval()
            .expect("data");
        assert_eq!(data, data_dir.join("fonts").join("f").to_string_lossy());
        let confroot: String = lua
            .load(r#"return confit.path.confroot("templates", "app.conf.j2")"#)
            .eval()
            .expect("confroot");
        assert_eq!(
            confroot,
            root_dir
                .path()
                .join("templates")
                .join("app.conf.j2")
                .to_string_lossy()
        );
    }

    #[test]
    fn lua_helpers_without_segments_return_bases() {
        let home_dir = tempfile::tempdir().expect("home tempdir");
        let config_dir = home_dir.path().join("cfg");
        let data_dir = home_dir.path().join("share");
        std::fs::create_dir_all(&config_dir).expect("mkdir config");
        std::fs::create_dir_all(&data_dir).expect("mkdir data");
        let root_dir = tempfile::tempdir().expect("root tempdir");
        let (_lock, _restore) = pin_env(home_dir.path(), &config_dir, &data_dir);

        let lua = setup(root_dir.path());
        let home: String = lua
            .load(r#"return confit.path.home()"#)
            .eval()
            .expect("home");
        assert_eq!(home, home_dir.path().to_string_lossy());
        let config: String = lua
            .load(r#"return confit.path.config()"#)
            .eval()
            .expect("config");
        assert_eq!(config, config_dir.to_string_lossy());
        let data: String = lua
            .load(r#"return confit.path.data()"#)
            .eval()
            .expect("data");
        assert_eq!(data, data_dir.to_string_lossy());
        let confroot: String = lua
            .load(r#"return confit.path.confroot()"#)
            .eval()
            .expect("confroot");
        assert_eq!(confroot, root_dir.path().to_string_lossy());
    }

    #[test]
    fn non_string_segments_fail_naming_function() {
        let home_dir = tempfile::tempdir().expect("home tempdir");
        let config_dir = home_dir.path().join("cfg");
        let data_dir = home_dir.path().join("share");
        std::fs::create_dir_all(&config_dir).expect("mkdir config");
        std::fs::create_dir_all(&data_dir).expect("mkdir data");
        let root_dir = tempfile::tempdir().expect("root tempdir");
        let (_lock, _restore) = pin_env(home_dir.path(), &config_dir, &data_dir);

        let lua = setup(root_dir.path());
        for (func, expr) in [
            ("home", r#"return confit.path.home("ok", 42)"#),
            ("config", r#"return confit.path.config(42)"#),
            ("data", r#"return confit.path.data({})"#),
            ("confroot", r#"return confit.path.confroot("ok", true)"#),
        ] {
            let err = lua.load(expr).eval::<String>().expect_err("must fail");
            let message = err.to_string();
            assert!(message.contains(func), "error names {func}: {message}");
        }
    }
}
