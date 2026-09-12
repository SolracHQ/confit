//! Render
//!
//! Data-to-bytes rendering for every artifact kind.

/// Shell rc text rendering with per-tool markers.
pub mod rc;
/// Bourne shell argument quoting.
pub mod shell_escape;
/// Table serializers for TOML, JSON, and YAML.
pub mod structured;

use std::collections::BTreeMap;
use std::path::Path;

use crate::error::{Error, Result};
use crate::model::state::artifact::ArtifactData;
use crate::model::state::artifact::BlameSet;
use crate::model::state::artifact::Table;
use crate::model::state::plan::Plan;
use crate::repository::Filesystem;

use crate::services::path::resolve_src;

/// Renders resolved template text, mapping syntax failures to plan errors.
fn render_template(text: &str, vars: &Table) -> Result<String> {
    minijinja::Environment::new()
        .render_str(text, vars)
        .map_err(|e| Error::Plan(format!("render template: {e}")))
}

/// Renders artifact data to on-disk bytes.
///
/// # Arguments
///
/// * `data` - the merged payload.
/// * `blame` - per-entry winners for rc sections.
/// * `files` - reads `src` fields.
/// * `root` - the project root for template `src` reads.
///
/// # Returns
///
/// Exact bytes landing on disk for the artifact.
///
/// # Errors
///
/// Template syntax failures and TOML null values yield plan errors. Escapes yield plan errors.
/// Template reads yield store errors.
///
/// # Examples
/// ```rust
/// use std::path::Path;
/// use confit::model::state::artifact::ArtifactData;
/// use confit::model::state::artifact::BlameSet;
/// use confit::repository::MemoryFilesystem;
/// use confit::services::render::render_artifact;
///
/// let source = MemoryFilesystem::default();
/// let data = ArtifactData::File { content: "hi".into() };
/// assert!(matches!(render_artifact(&data, &BlameSet::default(), &source, Path::new("root")), Ok(bytes) if bytes == b"hi"));
/// ```
pub fn render_artifact(
    data: &ArtifactData,
    blame: &BlameSet,
    files: &dyn Filesystem,
    root: &Path,
) -> Result<Vec<u8>> {
    match data {
        ArtifactData::Toml(table) => Ok(structured::render_toml(table)?.into_bytes()),
        ArtifactData::Json(table) => Ok(structured::render_json(table)?.into_bytes()),
        ArtifactData::Yaml(table) => Ok(structured::render_yaml(table)?.into_bytes()),
        ArtifactData::Template { src, vars } => {
            let candidate = resolve_src(root, src)?;
            let text = match files.read_string(&candidate.display().to_string())? {
                Some(text) => text,
                None => src.clone(),
            };
            Ok(render_template(&text, vars)?.into_bytes())
        }
        ArtifactData::File { content } => Ok(content.as_bytes().to_vec()),
        ArtifactData::Link { target } => Ok(target.as_bytes().to_vec()),
        ArtifactData::Rc(rc) => Ok(rc::render_rc(rc, blame).into_bytes()),
    }
}

/// Renders baseline bytes for every planned artifact.
///
/// # Arguments
///
/// * `plan` - plan holding artifacts for rendering.
/// * `root` - root holding template sources.
/// * `files` - reads template `src` fields.
///
/// # Returns
///
/// Rendered bytes keyed by kind plus path.
///
/// # Errors
///
/// Fails with render errors from file reads.
pub fn render_baseline(
    plan: &Plan,
    root: &Path,
    files: &dyn Filesystem,
) -> Result<BTreeMap<String, Vec<u8>>> {
    let mut rendered = BTreeMap::new();
    for artifact in &plan.artifacts {
        let key = artifact.key_string();
        let bytes = render_artifact(&artifact.data, &artifact.blame, files, root)?;
        rendered.insert(key, bytes);
    }
    Ok(rendered)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::model::state::artifact::Table;
    use crate::repository::MemoryFilesystem;

    fn table(pairs: &[(&str, serde_json::Value)]) -> Table {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_string(), value.clone()))
            .collect()
    }

    #[test]
    fn dispatch_covers_every_kind() {
        let source = MemoryFilesystem::default();
        let root = Path::new("root");
        let cases: Vec<(ArtifactData, Vec<u8>)> = vec![
            (
                ArtifactData::Toml(table(&[("a", serde_json::json!(1))])),
                b"a = 1\n".to_vec(),
            ),
            (
                ArtifactData::Json(table(&[("a", serde_json::json!(1))])),
                b"{\n  \"a\": 1\n}".to_vec(),
            ),
            (
                ArtifactData::Yaml(table(&[("a", serde_json::json!(1))])),
                b"a: 1".to_vec(),
            ),
            (
                ArtifactData::Template {
                    src: "hi {{ who }}".into(),
                    vars: table(&[("who", serde_json::json!("you"))]),
                },
                b"hi you".to_vec(),
            ),
            (
                ArtifactData::File {
                    content: "raw".into(),
                },
                b"raw".to_vec(),
            ),
            (
                ArtifactData::File {
                    content: String::new(),
                },
                Vec::new(),
            ),
            (
                ArtifactData::Link {
                    target: "dest".into(),
                },
                b"dest".to_vec(),
            ),
            (
                ArtifactData::Link {
                    target: String::new(),
                },
                Vec::new(),
            ),
        ];
        for (data, expected) in cases {
            assert_eq!(
                render_artifact(&data, &BlameSet::default(), &source, root).unwrap(),
                expected
            );
        }
    }

    #[test]
    fn rc_dispatch_uses_unknown_owner() {
        let source = MemoryFilesystem::default();
        let data = ArtifactData::Rc(crate::model::state::rc::RcData {
            profile: Vec::new(),
            env: vec![crate::model::state::rc::EnvEntry {
                name: "A".into(),
                value: "1".into(),
                when: None,
            }],
            aliases: Table::new()
                .into_iter()
                .map(|(key, value)| (key, value.to_string()))
                .collect(),
            init: Vec::new(),
        });
        assert_eq!(
            render_artifact(&data, &BlameSet::default(), &source, Path::new("root")).unwrap(),
            b"# >>> confit:unknown\nexport A=1\n# <<< confit\n"
        );
    }

    #[test]
    fn syntax_error_is_plan_error() {
        let error = render_template("{{ unclosed", &table(&[])).unwrap_err();
        assert!(matches!(error, Error::Plan(_)), "{error}");
    }

    #[test]
    fn memory_hit_renders_registered_key() {
        let files = MemoryFilesystem::default();
        files.files.borrow_mut().insert(
            Path::new("root").join("app.conf").display().to_string(),
            b"stored {{ who }}".to_vec(),
        );
        let data = ArtifactData::Template {
            src: "app.conf".into(),
            vars: table(&[("who", serde_json::json!("you"))]),
        };
        assert_eq!(
            render_artifact(&data, &BlameSet::default(), &files, Path::new("root")).unwrap(),
            b"stored you"
        );
    }

    #[test]
    fn hash_matches_known_vectors() {
        assert_eq!(
            crate::security::sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            crate::security::sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
