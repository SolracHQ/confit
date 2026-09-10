//! Canonical bytes and hashes for merged artifacts.
//!
//! Guarantees byte-identical output for equal data on every run, plus a
//! plan id untouched by artifact order. Merge losers (`_shadowed`), per-entry
//! blame (`_blame`), and plan metadata stay outside the digest, so debug-only
//! edits keep winner identity.

use sha2::{Digest, Sha256};

use crate::error::Result;
use crate::model::ArtifactData;

/// Canonical JSON bytes of merged artifact data.
///
/// Guarantees byte-identical output for equal data on every run: key
/// order is fixed, list order follows declaration order. Args: `data`
/// holds the merged payload. Returns the exact bytes [`data_hash`]
/// digests.
pub fn canonical_bytes(data: &ArtifactData) -> Result<Vec<u8>> {
    Ok(serde_json::to_vec(data)?)
}

/// Hex sha256 over [`canonical_bytes`].
///
/// Yields the content hash of merged winners.
/// Guarantees: equal data shares one hash; every winner byte feeds the
/// digest while `_shadowed` losers and `_blame` attribution stay outside
/// it, so blame-only and loser-only edits keep winner identity.
///
/// Args: `data` holds the merged payload.
///
/// Example:
/// ```rust
/// use confit::canonical::data_hash;
/// use confit::model::{ArtifactData, Table};
///
/// let data = ArtifactData::Toml(Table::new());
/// assert_eq!(data_hash(&data).unwrap(), data_hash(&data).unwrap());
/// ```
pub fn data_hash(data: &ArtifactData) -> Result<String> {
    Ok(hex_digest(&canonical_bytes(data)?))
}

/// Plan id over per-artifact hashes.
///
/// Guarantees: artifact order leaves the id untouched; the id changes
/// exactly when the artifact hash set changes.
///
/// Args: `hashes` hold the per-artifact hex digests.
pub fn plan_hash(hashes: &[String]) -> String {
    let mut sorted = hashes.to_vec();
    sorted.sort();
    hex_digest(sorted.concat().as_bytes())
}

/// Produces hex sha256 of raw bytes.
fn hex_digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::artifact::BlameSet;
    use crate::model::{Artifact, ArtifactKind, Contribution, EnvEntry, Shadowed, ShadowedSet};

    fn file_artifact(content: &str) -> Artifact {
        Artifact {
            kind: ArtifactKind::File,
            path: "p".into(),
            data: ArtifactData::File {
                content: content.into(),
            },
            contributions: vec![Contribution {
                tool: "a".into(),
                order: 0,
            }],
            shadowed: ShadowedSet::default(),
            blame: BlameSet::default(),
            data_hash: String::new(),
        }
    }

    #[test]
    fn hash_is_stable_for_same_data() {
        let data = ArtifactData::Template {
            src: "hello {{ name }}".into(),
            vars: [("name".to_string(), serde_json::json!("world"))]
                .into_iter()
                .collect(),
        };
        assert_eq!(
            canonical_bytes(&data).unwrap(),
            canonical_bytes(&data).unwrap()
        );
        assert_eq!(data_hash(&data).unwrap(), data_hash(&data).unwrap());
        assert_eq!(data_hash(&data).unwrap().len(), 64);
    }

    #[test]
    fn shadowed_entries_do_not_affect_hash() {
        let plain = file_artifact("bytes");
        let mut shadowed_artifact = file_artifact("bytes");
        shadowed_artifact.shadowed.env.push(Shadowed {
            by_tool: "zoxide".into(),
            reason: "same name, structurally equal when".into(),
            entry: EnvEntry {
                name: "_ZO_DOCTOR".into(),
                value: "0".into(),
                when: None,
            },
            winner: "zoxide".into(),
        });
        assert!(!shadowed_artifact.shadowed.is_empty());
        assert_eq!(
            data_hash(&plain.data).unwrap(),
            data_hash(&shadowed_artifact.data).unwrap()
        );
    }

    #[test]
    fn blame_and_shadowed_stay_outside_hash() {
        let plain = file_artifact("bytes");
        let mut attributed = file_artifact("bytes");
        attributed.shadowed.env.push(Shadowed {
            by_tool: "zoxide".into(),
            reason: "same name, structurally equal when".into(),
            entry: EnvEntry {
                name: "_ZO_DOCTOR".into(),
                value: "0".into(),
                when: None,
            },
            winner: "zoxide".into(),
        });
        attributed.blame.env.push("zoxide".into());
        attributed
            .blame
            .toml
            .insert("tools.bat".into(), "mise-tool".into());
        assert!(!attributed.shadowed.is_empty());
        assert!(!attributed.blame.is_empty());
        assert_eq!(
            data_hash(&plain.data).unwrap(),
            data_hash(&attributed.data).unwrap()
        );
        let plain_value = serde_json::to_value(&plain).unwrap();
        assert!(plain_value.get("_blame").is_none());
        let attributed_value = serde_json::to_value(&attributed).unwrap();
        assert!(attributed_value.get("_blame").is_some());
        assert!(attributed_value.get("_shadowed").is_some());
    }

    #[test]
    fn plan_hash_ignores_artifact_order() {
        let first = "aaa".to_string();
        let second = "bbb".to_string();
        assert_eq!(
            plan_hash(&[first.clone(), second.clone()]),
            plan_hash(&[second.clone(), first.clone()])
        );
        assert_ne!(
            plan_hash(std::slice::from_ref(&first)),
            plan_hash(&[first, second])
        );
    }
}
