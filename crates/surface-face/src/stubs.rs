//! Stubs
//! Stub proof for the Lua surface.

use std::path::{Path, PathBuf};

use mlua::Lua;
use mlua_extras::typed::Type;
use mlua_extras::typed::generator::Definition;
use thiserror::Error;

/// Stub generation failure shapes.
#[derive(Debug, Error)]
pub enum StubError {
    /// Stub file write failure.
    #[error("cannot write stub '{path}': {reason}")]
    Write {
        /// The stub file path.
        path: String,
        /// The nested failure reason.
        reason: String,
    },
    /// Stub parse failure.
    #[error("stub '{path}' fails to parse: {reason}")]
    Parse {
        /// The stub file path.
        path: String,
        /// The nested failure reason.
        reason: String,
    },
}

/// Stub result alias.
pub type Result<T> = std::result::Result<T, StubError>;

/// Write the live stub definitions under one folder.
///
/// - All files parse as Lua on write.
///
/// # Errors
///
/// - [`StubError::Write`] for failed stub file writes.
/// - [`StubError::Parse`] for stubs failing the Lua parse gate.
pub fn write(dir: &Path, groups: &[(String, Definition)]) -> Result<Vec<PathBuf>> {
    use mlua_extras::typed::generator::{DefinitionFileGenerator, Definitions};
    let mut written = Vec::new();
    for (module, definition) in groups {
        let single = Definitions::start()
            .define(module, definition.clone())
            .finish();
        let generator = DefinitionFileGenerator::new(single);
        for (name, writer) in generator.iter() {
            let path = dir.join(&name);
            writer.write_file(&path).map_err(|error| StubError::Write {
                path: path.display().to_string(),
                reason: error.to_string(),
            })?;
            name_meta(&path)?;
            append_return(&path, definition)?;
            gate(&path)?;
            written.push(path);
        }
    }
    Ok(written)
}

/// Name one stub meta header after its module file.
///
/// # Errors
///
/// - [`StubError::Write`] for unreadable or unwritable stub files.
fn name_meta(path: &Path) -> Result<()> {
    let module = path
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.strip_suffix(".d.lua"))
        .unwrap_or("unknown");
    let text = std::fs::read_to_string(path).map_err(|error| StubError::Write {
        path: path.display().to_string(),
        reason: error.to_string(),
    })?;
    let named = text.replacen("--- @meta\n", &format!("--- @meta {module}\n"), 1);
    std::fs::write(path, named).map_err(|error| StubError::Write {
        path: path.display().to_string(),
        reason: error.to_string(),
    })
}

/// Append the module return statement with its values.
///
/// One function takes a bare return, several take a table, and one bare
/// value takes a bare return.
///
/// # Errors
///
/// - [`StubError::Write`] for unreadable or unwritable stub files.
fn append_return(path: &Path, definition: &Definition) -> Result<()> {
    let mut functions = Vec::new();
    let mut values = Vec::new();
    for entry in definition.iter() {
        match &entry.ty {
            Type::Function { .. } => functions.push(entry.name.clone()),
            Type::Proxy(_) => values.push(entry.name.clone()),
            _ => {}
        }
    }
    let statement = if functions.len() == 1 && values.is_empty() {
        format!("return {}", functions[0])
    } else if functions.is_empty() {
        if values.len() == 1 {
            format!("return {}", values[0])
        } else {
            return Ok(());
        }
    } else {
        let fields = functions
            .iter()
            .map(|name| format!("{name} = {name}"))
            .collect::<Vec<_>>()
            .join(", ");
        format!("return {{{fields}}}")
    };
    let mut text = std::fs::read_to_string(path).map_err(|error| StubError::Write {
        path: path.display().to_string(),
        reason: error.to_string(),
    })?;
    text.push_str(&format!("\n{statement}\n"));
    std::fs::write(path, text).map_err(|error| StubError::Write {
        path: path.display().to_string(),
        reason: error.to_string(),
    })
}

/// Gate one stub file through the Lua parser.
///
/// # Errors
///
/// - [`StubError::Parse`] for stubs failing the Lua parse gate.
fn gate(path: &Path) -> Result<()> {
    let text = std::fs::read_to_string(path).map_err(|error| StubError::Parse {
        path: path.display().to_string(),
        reason: error.to_string(),
    })?;
    let lua = Lua::new();
    lua.load(&text)
        .set_name(format!("@{}", path.display()))
        .exec()
        .map_err(|error| StubError::Parse {
            path: path.display().to_string(),
            reason: error.to_string(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mlua_extras::typed::generator::Definition;

    fn live_sample() -> Vec<(String, Definition)> {
        vec![(
            "confit.version".to_owned(),
            Definition::start()
                .document("Api level of the Lua surface, one integer.")
                .proxy::<i64>("version")
                .finish(),
        )]
    }

    #[test]
    fn write_lands_stubs() {
        let dir = tempfile::tempdir().unwrap();
        let written = match write(dir.path(), &live_sample()) {
            Ok(written) => written,
            Err(error) => panic!("stubs write: {error}"),
        };
        assert_eq!(written.len(), 1, "the write lands one file per module");
        for path in &written {
            assert!(
                path.starts_with(dir.path()),
                "the stub lands under the folder: {}",
                path.display()
            );
            let text = std::fs::read_to_string(path).unwrap();
            let module = path
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .strip_suffix(".d.lua")
                .unwrap();
            assert!(
                text.starts_with(&format!("--- @meta {module}\n")),
                "meta shows the module: {}",
                path.display()
            );
            assert!(
                text.contains("\nreturn "),
                "stub closes with its return: {}",
                path.display()
            );
        }
    }
}
