use std::path::{Path, PathBuf};

use confit_binding::registry::Registry;
use confit_binding::stub::StubEmitter;

fn main() {
    if let Err(error) = run() {
        eprintln!("stubs gen: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let dir = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/stubs"));
    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let mut written = Vec::new();
    for document in documents()? {
        let path = dir.join(format!("{}.d.lua", document.namespace));
        std::fs::write(&path, &document.text)
            .map_err(|error| format!("cannot write stub '{}': {error}", path.display()))?;
        check_parse(&path)?;
        written.push(path);
    }
    for path in written {
        println!("{}", path.display());
    }
    Ok(())
}

struct StubDocument {
    namespace: String,
    text: String,
}

fn documents() -> Result<Vec<StubDocument>, String> {
    Ok(vec![
        module_document(
            confit_engine::api::fetch::Module::NAMESPACE,
            confit_engine::api::fetch::Module::VERBS,
            confit_engine::api::fetch::Module::stubs,
        )?,
        module_document(
            confit_engine::api::resource::Module::NAMESPACE,
            confit_engine::api::resource::Module::VERBS,
            confit_engine::api::resource::Module::stubs,
        )?,
        module_document(
            confit_engine::api::validate::Module::NAMESPACE,
            confit_engine::api::validate::Module::VERBS,
            confit_engine::api::validate::Module::stubs,
        )?,
        module_document(
            confit_engine::api::fact::Module::NAMESPACE,
            confit_engine::api::fact::Module::VERBS,
            confit_engine::api::fact::Module::stubs,
        )?,
        module_document(
            confit_engine::api::template::Module::NAMESPACE,
            confit_engine::api::template::Module::VERBS,
            confit_engine::api::template::Module::stubs,
        )?,
    ])
}

/// Build one stub document from its module face with the mechanical return line.
fn module_document(
    namespace: &'static str,
    verbs: &[&str],
    collect: fn(&mut Registry) -> confit_binding::error::Result<()>,
) -> Result<StubDocument, String> {
    let mut registry = Registry::new();
    collect(&mut registry).map_err(|error| error.to_string())?;
    Ok(emit(namespace, &registry, &return_line(verbs)))
}

/// Build the mechanical return line for one verb list with single verbs
/// returning the verb name and verb lists returning the verb table.
fn return_line(verbs: &[&str]) -> String {
    match verbs {
        [single] => format!("return {single}"),
        _ => {
            let entries: Vec<String> = verbs
                .iter()
                .map(|verb| format!("{verb} = {verb}"))
                .collect();
            format!("return {{{}}}", entries.join(", "))
        }
    }
}

fn emit(namespace: &str, registry: &Registry, statement: &str) -> StubDocument {
    let emitter = StubEmitter::new(format!("--- @meta {namespace}"));
    let mut document = emitter.emit(namespace, registry);
    document.text.push('\n');
    document.text.push_str(statement);
    document.text.push('\n');
    StubDocument {
        namespace: document.namespace,
        text: document.text,
    }
}

fn check_parse(path: &Path) -> Result<(), String> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("stub '{}' fails to parse: {error}", path.display()))?;
    let lua = mlua::Lua::new();
    lua.load(&text)
        .set_name(format!("@{}", path.display()))
        .exec()
        .map_err(|error| format!("stub '{}' fails to parse: {error}", path.display()))
}
