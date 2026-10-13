//! Core plugins.

use std::env;
use std::fs;
use std::path::Path;
use std::path::PathBuf;

fn main() {
    if let Err(error) = run() {
        eprintln!("cli build: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let manifest =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").map_err(|error| error.to_string())?);
    let root = manifest
        .parent()
        .and_then(|path| path.parent())
        .ok_or_else(|| "cli build: workspace root misses above the manifest".to_string())?;
    let plugins = root.join("plugins");
    if !plugins.is_dir() {
        return Err(format!(
            "cli build: plugins folder misses at '{}'",
            plugins.display()
        ));
    }
    println!("cargo::rerun-if-changed={}", plugins.display());
    let mut entries = Vec::new();
    collect(&plugins, &plugins, &mut entries)?;
    entries.sort();
    let out = PathBuf::from(env::var("OUT_DIR").map_err(|error| error.to_string())?);
    let mut text = String::from("// Core plugin files, keyed under the plugin folder.\n");
    text.push_str("const PLUGIN_FILES: &[(&str, &str)] = &[\n");
    for (key, _) in &entries {
        let rel = key.strip_prefix("plugins/").unwrap_or(key);
        text.push_str(&format!(
            "    ({rel:?}, include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/../../{key}\"))),\n"
        ));
    }
    text.push_str("];\n");
    fs::write(out.join("plugin_files.rs"), text).map_err(|error| error.to_string())?;
    Ok(())
}

/// Core plugin files with target paths.
fn collect(dir: &Path, plugins: &Path, entries: &mut Vec<(String, PathBuf)>) -> Result<(), String> {
    let read = fs::read_dir(dir)
        .map_err(|error| format!("cli build: cannot list '{}': {error}", dir.display()))?;
    for entry in read {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        if path.is_dir() {
            collect(&path, plugins, entries)?;
        } else if path.extension().is_some_and(|ext| ext == "lua") {
            let key = path
                .strip_prefix(plugins)
                .map_err(|error| error.to_string())?;
            let key = key
                .components()
                .map(|part| {
                    part.as_os_str()
                        .to_str()
                        .ok_or_else(|| {
                            format!("cli build: path name misses text at '{}'", path.display())
                        })
                        .map(str::to_string)
                })
                .collect::<Result<Vec<_>, _>>()?
                .join("/");
            println!("cargo::rerun-if-changed={}", path.display());
            entries.push((format!("plugins/{key}"), path));
        }
    }
    Ok(())
}
