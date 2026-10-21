//! `.ariaplugin`: one signed file that carries a plugin and its extras.
//!
//! Layout (all integers little-endian):
//!
//! ```text
//! "ARIAPLUG" | format: u16 = 1 | header length: u32 | header (JSON)
//!   | file bytes, in header order | Ed25519 signature (64 bytes)
//! ```
//!
//! The header names the signing key and lists every file with its length
//! and BLAKE3 hash. The signature covers everything before it, prefixed by
//! a domain string, so a package cannot be re-signed as a PAK or vice versa.
//! `plugin.wasm` is required; `ui.js` (a Web UI module) and `assets/…` are
//! optional and are handed to the host untouched.

use std::collections::BTreeMap;

use aria_plugin_sdk::PluginRegistration;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{PluginLoadError, load_plugin};

const MAGIC: &[u8; 8] = b"ARIAPLUG";
const FORMAT: u16 = 1;
const SIGNATURE_DOMAIN: &[u8] = b"aria-plugin-package-v1\0";
const PREFIX_LEN: usize = MAGIC.len() + 2 + 4;
const SIGNATURE_LEN: usize = 64;
pub const MAX_PACKAGE_BYTES: usize = 16 * 1024 * 1024;
const MAX_HEADER_BYTES: usize = 64 * 1024;
const MAX_FILES: usize = 64;
pub const WASM_FILE: &str = "plugin.wasm";
pub const UI_MODULE_FILE: &str = "ui.js";

#[derive(Debug, Error)]
pub enum PackageError {
    #[error("not an .ariaplugin file")]
    NotAPackage,
    #[error(".ariaplugin format {0} is unsupported (expected 1)")]
    Format(u16),
    #[error(".ariaplugin exceeds the 16 MiB limit")]
    TooLarge,
    #[error("malformed .ariaplugin: {0}")]
    Malformed(String),
    #[error("signing key '{0}' is not trusted by this game")]
    UntrustedKey(String),
    #[error(".ariaplugin signature does not match its contents")]
    BadSignature,
    #[error("file '{0}' does not match its recorded hash")]
    FileHash(String),
    #[error(transparent)]
    Plugin(#[from] PluginLoadError),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    key_id: String,
    files: Vec<FileEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileEntry {
    name: String,
    len: u64,
    blake3: String,
}

/// A verified package. `registration` comes from running the guest's
/// manifest export; `files` holds everything except `plugin.wasm`.
#[derive(Debug, Clone)]
pub struct PluginPackage {
    pub key_id: String,
    pub registration: PluginRegistration,
    pub wasm: Vec<u8>,
    pub files: BTreeMap<String, Vec<u8>>,
}

impl PluginPackage {
    #[must_use]
    pub fn ui_module(&self) -> Option<&[u8]> {
        self.files.get(UI_MODULE_FILE).map(Vec::as_slice)
    }
}

/// Builds a signed package. `extra` may hold `ui.js` and `assets/…` files.
pub fn pack(
    wasm: &[u8],
    extra: &BTreeMap<String, Vec<u8>>,
    key_id: &str,
    signing_key: &[u8; 32],
) -> Result<Vec<u8>, PackageError> {
    // Refuse to sign something the loader would reject later.
    load_plugin(wasm)?;
    if !valid_key_id(key_id) {
        return Err(PackageError::Malformed(format!(
            "invalid key id '{key_id}'"
        )));
    }
    let mut files = vec![(WASM_FILE, wasm)];
    for (name, bytes) in extra {
        if name == WASM_FILE {
            return Err(PackageError::Malformed("plugin.wasm is given twice".into()));
        }
        files.push((name.as_str(), bytes.as_slice()));
    }
    let header = Header {
        key_id: key_id.to_owned(),
        files: files
            .iter()
            .map(|(name, bytes)| FileEntry {
                name: (*name).to_owned(),
                len: bytes.len() as u64,
                blake3: blake3::hash(bytes).to_hex().to_string(),
            })
            .collect(),
    };
    validate_header(&header)?;
    let header = serde_json::to_vec(&header).map_err(|e| PackageError::Malformed(e.to_string()))?;
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&FORMAT.to_le_bytes());
    out.extend_from_slice(&(header.len() as u32).to_le_bytes());
    out.extend_from_slice(&header);
    for (_, bytes) in &files {
        out.extend_from_slice(bytes);
    }
    let signature = SigningKey::from_bytes(signing_key).sign(&signed_message(&out));
    out.extend_from_slice(&signature.to_bytes());
    if out.len() > MAX_PACKAGE_BYTES {
        return Err(PackageError::TooLarge);
    }
    Ok(out)
}

/// Verifies a package against the game's trusted keys and loads its guest.
/// Performs no I/O; hosts read the bytes from wherever the file lives.
pub fn open_package(
    bytes: &[u8],
    trusted_keys: &BTreeMap<String, [u8; 32]>,
) -> Result<PluginPackage, PackageError> {
    if bytes.len() > MAX_PACKAGE_BYTES {
        return Err(PackageError::TooLarge);
    }
    if bytes.len() < PREFIX_LEN + SIGNATURE_LEN || &bytes[..MAGIC.len()] != MAGIC {
        return Err(PackageError::NotAPackage);
    }
    let format = u16::from_le_bytes([bytes[8], bytes[9]]);
    if format != FORMAT {
        return Err(PackageError::Format(format));
    }
    let header_len = u32::from_le_bytes([bytes[10], bytes[11], bytes[12], bytes[13]]) as usize;
    let content_end = bytes.len() - SIGNATURE_LEN;
    if header_len > MAX_HEADER_BYTES || PREFIX_LEN + header_len > content_end {
        return Err(PackageError::Malformed("header length out of range".into()));
    }
    let header: Header = serde_json::from_slice(&bytes[PREFIX_LEN..PREFIX_LEN + header_len])
        .map_err(|e| PackageError::Malformed(e.to_string()))?;
    validate_header(&header)?;

    // Check the signature before trusting any recorded length or hash.
    let key = trusted_keys
        .get(&header.key_id)
        .ok_or_else(|| PackageError::UntrustedKey(header.key_id.clone()))?;
    let key = VerifyingKey::from_bytes(key).map_err(|_| PackageError::BadSignature)?;
    let signature: [u8; SIGNATURE_LEN] = bytes[content_end..]
        .try_into()
        .expect("slice has the signature length");
    key.verify(
        &signed_message(&bytes[..content_end]),
        &Signature::from_bytes(&signature),
    )
    .map_err(|_| PackageError::BadSignature)?;

    let mut offset = PREFIX_LEN + header_len;
    let mut wasm = None;
    let mut files = BTreeMap::new();
    for entry in &header.files {
        let len = usize::try_from(entry.len)
            .map_err(|_| PackageError::Malformed("file length out of range".into()))?;
        let end = offset
            .checked_add(len)
            .filter(|end| *end <= content_end)
            .ok_or_else(|| PackageError::Malformed("file runs past the end".into()))?;
        let content = &bytes[offset..end];
        if blake3::hash(content).to_hex().as_str() != entry.blake3 {
            return Err(PackageError::FileHash(entry.name.clone()));
        }
        if entry.name == WASM_FILE {
            wasm = Some(content.to_vec());
        } else {
            files.insert(entry.name.clone(), content.to_vec());
        }
        offset = end;
    }
    if offset != content_end {
        return Err(PackageError::Malformed(
            "trailing bytes before the signature".into(),
        ));
    }
    let wasm = wasm.ok_or_else(|| PackageError::Malformed("plugin.wasm is missing".into()))?;
    let registration = load_plugin(&wasm)?;
    Ok(PluginPackage {
        key_id: header.key_id,
        registration,
        wasm,
        files,
    })
}

fn signed_message(content: &[u8]) -> Vec<u8> {
    let mut message = Vec::with_capacity(SIGNATURE_DOMAIN.len() + content.len());
    message.extend_from_slice(SIGNATURE_DOMAIN);
    message.extend_from_slice(content);
    message
}

fn validate_header(header: &Header) -> Result<(), PackageError> {
    if !valid_key_id(&header.key_id) {
        return Err(PackageError::Malformed("invalid key id".into()));
    }
    if header.files.is_empty() || header.files.len() > MAX_FILES {
        return Err(PackageError::Malformed(
            "a package holds 1..=64 files".into(),
        ));
    }
    let mut names = std::collections::BTreeSet::new();
    for entry in &header.files {
        if !valid_file_name(&entry.name) || !names.insert(entry.name.as_str()) {
            return Err(PackageError::Malformed(format!(
                "invalid or duplicate file name '{}'",
                entry.name
            )));
        }
        if entry.blake3.len() != 64 || !entry.blake3.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(PackageError::Malformed("invalid file hash".into()));
        }
    }
    if !names.contains(WASM_FILE) {
        return Err(PackageError::Malformed("plugin.wasm is missing".into()));
    }
    Ok(())
}

fn valid_key_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// `plugin.wasm`, `ui.js`, or `assets/<path>` with lowercase segments.
fn valid_file_name(name: &str) -> bool {
    if name == WASM_FILE || name == UI_MODULE_FILE {
        return true;
    }
    let Some(rest) = name.strip_prefix("assets/") else {
        return false;
    };
    name.len() <= 200
        && rest.split('/').all(|segment| {
            !segment.is_empty()
                && segment != "."
                && segment != ".."
                && segment.bytes().all(|b| {
                    b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-')
                })
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::guest_with_manifest;

    const KEY: [u8; 32] = [7; 32];

    fn trusted() -> BTreeMap<String, [u8; 32]> {
        let public = SigningKey::from_bytes(&KEY).verifying_key().to_bytes();
        [("publisher".to_owned(), public)].into()
    }

    fn wasm() -> Vec<u8> {
        guest_with_manifest(
            r#"{"api_version":2,"id":"test.slide","version":"1.2.0","capabilities":["routes"],"routes":[{"route":"slide"}]}"#,
        )
    }

    #[test]
    fn a_signed_package_round_trips_with_its_extras() {
        let extra = BTreeMap::from([
            (UI_MODULE_FILE.to_owned(), b"export default {}".to_vec()),
            ("assets/frame.png".to_owned(), vec![1, 2, 3]),
        ]);
        let bytes = pack(&wasm(), &extra, "publisher", &KEY).unwrap();
        let package = open_package(&bytes, &trusted()).unwrap();
        assert_eq!(package.key_id, "publisher");
        assert_eq!(package.registration.manifest.id, "test.slide");
        assert_eq!(package.wasm, wasm());
        assert_eq!(package.ui_module(), Some(&b"export default {}"[..]));
        assert_eq!(package.files["assets/frame.png"], vec![1, 2, 3]);
    }

    #[test]
    fn tampering_an_unknown_key_and_bad_names_are_refused() {
        let bytes = pack(&wasm(), &BTreeMap::new(), "publisher", &KEY).unwrap();
        let mut tampered = bytes.clone();
        let middle = tampered.len() / 2;
        tampered[middle] ^= 1;
        assert!(matches!(
            open_package(&tampered, &trusted()),
            Err(PackageError::BadSignature | PackageError::Malformed(_))
        ));
        let other = pack(&wasm(), &BTreeMap::new(), "someone", &[9; 32]).unwrap();
        assert!(matches!(
            open_package(&other, &trusted()),
            Err(PackageError::UntrustedKey(key)) if key == "someone"
        ));
        let forged = {
            let mut keys = trusted();
            keys.insert("someone".into(), *keys.get("publisher").unwrap());
            open_package(&other, &keys)
        };
        assert!(matches!(forged, Err(PackageError::BadSignature)));
        for name in ["../escape", "assets/../x", "assets/Upper.png", "readme.txt"] {
            let extra = BTreeMap::from([(name.to_owned(), vec![0])]);
            assert!(pack(&wasm(), &extra, "publisher", &KEY).is_err(), "{name}");
        }
        assert!(matches!(
            open_package(
                b"not a plugin at all, just text with enough length to pass",
                &trusted()
            ),
            Err(PackageError::NotAPackage)
        ));
    }
}
