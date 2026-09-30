//! Bundle input gate holding manifest parse and member checks.

use confit_model::manifest::{MANIFEST_VERSION, Manifest};
use thiserror::Error;

/// Blob id length in lowercase hex chars.
const BLOB_ID_LEN: usize = 64;

/// Manifest codec failure shapes.
#[derive(Debug, Error)]
pub enum CodecError {
    /// Corrupt payload holding the parse reason.
    #[error("cannot read manifest: {message}")]
    Corrupt {
        /// Holds the parse reason.
        message: String,
    },
    /// Unsupported version holding the seen version.
    #[error("manifest version {got} reads unsupported")]
    Version {
        /// Holds the seen version.
        got: u64,
    },
}

/// Parses one manifest with the version gate first.
///
/// # Errors
///
/// - [`CodecError::Corrupt`] for bad payloads.
/// - [`CodecError::Version`] for stale versions.
pub fn decode(bytes: &[u8]) -> Result<Manifest, CodecError> {
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|error| CodecError::Corrupt {
            message: error.to_string(),
        })?;
    if value
        .get("version")
        .and_then(serde_json::Value::as_u64)
        .is_none()
    {
        return Err(CodecError::Corrupt {
            message: "missing version".to_owned(),
        });
    }
    let manifest: Manifest =
        serde_json::from_value(value).map_err(|error| CodecError::Corrupt {
            message: error.to_string(),
        })?;
    if manifest.version != MANIFEST_VERSION {
        return Err(CodecError::Version {
            got: u64::from(manifest.version),
        });
    }
    Ok(manifest)
}

/// Checks one blob id holds 64 hex chars.
///
/// # Errors
///
/// - [`CodecError::Corrupt`] for malformed ids.
pub(crate) fn check_blob_id(sha: &str) -> Result<(), CodecError> {
    if sha.len() == BLOB_ID_LEN && sha.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(CodecError::Corrupt {
            message: "bad blob ref".to_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_refuses_stale_version_with_got() {
        let raw = serde_json::json!({"version": 0u32, "documents": []});
        let bytes = serde_json::to_vec(&raw).unwrap();
        match decode(&bytes) {
            Ok(_) => panic!("stale manifest passes"),
            Err(CodecError::Version { got }) => assert_eq!(got, 0, "stale keeps version"),
            Err(error) => panic!("wrong stale variant: {error}"),
        }
    }
}
