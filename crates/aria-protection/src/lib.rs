#![forbid(unsafe_code)]
#![deny(missing_debug_implementations)]

//! Protection and entitlement contracts that live outside `aria-core`.
//!
//! The deterministic Core consumes already-authorized asset bytes and never
//! sees keys, clocks, signatures, or network handles. Native and Web Players
//! use this crate for the common `.ariapak` envelope and for the intentionally
//! small two-operation [`LicenseProvider`] boundary.

use std::collections::BTreeMap;
use std::fmt;
use std::io::{Read, Write};

use aria_core::pak::{AssetInput, PakArchive, PakError};
use chacha20poly1305::aead::{AeadInPlace, KeyInit};
use chacha20poly1305::{Tag, XChaCha20Poly1305, XNonce};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use flate2::Compression;
use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// PAK4 successor envelope used by signed and protected role packs.
pub const PROTECTED_PAK_MAGIC: [u8; 8] = *b"ARIAPK4P";
/// The manifest and descriptor version carried by the envelope.
pub const PAK_FORMAT_VERSION: u16 = 4;
/// Maximum uncompressed package chunk. It matches the Core archive chunk
/// bound, so a protected wrapper never creates a much larger read unit.
pub const PAK_CHUNK_MAX_RAW_SIZE: usize = 256 * 1024;

const HEADER_SIZE: usize = 72;
const DESCRIPTOR_SIZE: usize = 112;
const SIGNATURE_SIZE: usize = 64;
const CODEC_STORED: u8 = 0;
const CODEC_DEFLATE: u8 = 1;
const FLAG_NONE: u8 = 0;

/// Distribution protection profile selected by the packager.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
#[serde(rename_all = "lowercase")]
pub enum PakProfile {
    /// Plaintext, integrity-checked development data.
    Dev,
    /// Plaintext data authenticated by the publisher's Ed25519 signature.
    Signed,
    /// Signed data whose individual chunks are encrypted with XChaCha20.
    Protected,
}

impl PakProfile {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Dev => "dev",
            Self::Signed => "signed",
            Self::Protected => "protected",
        }
    }

    #[must_use]
    pub const fn requires_signature(self) -> bool {
        matches!(self, Self::Signed | Self::Protected)
    }

    #[must_use]
    pub const fn requires_encryption(self) -> bool {
        matches!(self, Self::Protected)
    }

    const fn is_signed(self) -> bool {
        self.requires_signature()
    }

    const fn is_encrypted(self) -> bool {
        self.requires_encryption()
    }
}

/// Pack scheduling role. The number of packs is intentionally not fixed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PakRole {
    Boot,
    Hot,
    Cold,
    Overlay,
}

impl PakRole {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Boot => "boot",
            Self::Hot => "hot",
            Self::Cold => "cold",
            Self::Overlay => "overlay",
        }
    }
}

/// A dependency on another role pack. `subtype` is deliberately metadata
/// rather than another fixed role, so locale, patch, and DLC remain overlays.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PakDependency {
    pub pack_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minimum_version: Option<String>,
}

/// License behavior declared by a protected pack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LicensePolicy {
    pub required: bool,
    pub offline_allowed: bool,
    pub lease_seconds: u64,
    pub grace_seconds: u64,
}

impl LicensePolicy {
    #[must_use]
    pub const fn none() -> Self {
        Self {
            required: false,
            offline_allowed: false,
            lease_seconds: 0,
            grace_seconds: 0,
        }
    }

    #[must_use]
    pub const fn offline(lease_seconds: u64, grace_seconds: u64) -> Self {
        Self {
            required: true,
            offline_allowed: true,
            lease_seconds,
            grace_seconds,
        }
    }
}

/// The authenticated metadata carried by one `.ariapak` file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PakManifest {
    pub format_version: u16,
    pub pack_id: String,
    pub game_id: String,
    pub role: PakRole,
    pub subtype: String,
    pub dependencies: Vec<PakDependency>,
    pub priority: i32,
    pub content_root: String,
    pub archive_hash: String,
    pub profile: PakProfile,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature_key_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encryption_key_id: Option<String>,
    pub license_policy: LicensePolicy,
}

/// Validates a variable-size pack set before a Player mounts it. Dependencies
/// must resolve within the set and may not form a cycle; no role is assumed to
/// exist, so an empty `hot` or `cold` role is simply absent.
pub fn validate_pack_set(manifests: &[PakManifest]) -> Result<(), ProtectionError> {
    let mut by_id = BTreeMap::new();
    for manifest in manifests {
        validate_id(&manifest.pack_id, "pack ID")?;
        if by_id.insert(manifest.pack_id.as_str(), manifest).is_some() {
            return Err(ProtectionError::DuplicatePackId(manifest.pack_id.clone()));
        }
    }
    for manifest in manifests {
        for dependency in &manifest.dependencies {
            if dependency.pack_id == manifest.pack_id {
                return Err(ProtectionError::DependencyCycle(vec![
                    manifest.pack_id.clone(),
                ]));
            }
            if !by_id.contains_key(dependency.pack_id.as_str()) {
                return Err(ProtectionError::MissingDependency {
                    pack_id: manifest.pack_id.clone(),
                    dependency: dependency.pack_id.clone(),
                });
            }
        }
    }
    let mut visiting = BTreeMap::<&str, bool>::new();
    let mut visited = BTreeMap::<&str, bool>::new();
    for manifest in manifests {
        let mut path = Vec::new();
        visit_dependencies(
            manifest.pack_id.as_str(),
            &by_id,
            &mut visiting,
            &mut visited,
            &mut path,
        )?;
    }
    Ok(())
}

fn visit_dependencies<'a>(
    id: &'a str,
    by_id: &BTreeMap<&'a str, &'a PakManifest>,
    visiting: &mut BTreeMap<&'a str, bool>,
    visited: &mut BTreeMap<&'a str, bool>,
    path: &mut Vec<String>,
) -> Result<(), ProtectionError> {
    if visited.contains_key(id) {
        return Ok(());
    }
    if visiting.insert(id, true).is_some() {
        path.push(id.to_owned());
        return Err(ProtectionError::DependencyCycle(path.clone()));
    }
    path.push(id.to_owned());
    if let Some(manifest) = by_id.get(id) {
        for dependency in &manifest.dependencies {
            visit_dependencies(dependency.pack_id.as_str(), by_id, visiting, visited, path)?;
        }
    }
    let _ = path.pop();
    visiting.remove(id);
    visited.insert(id, true);
    Ok(())
}

/// A signing key supplied by a packager or license service. The secret bytes
/// are never included in Debug output or serialized manifest data.
#[derive(Clone)]
pub struct PakSigningKey {
    key_id: String,
    bytes: [u8; 32],
}

impl fmt::Debug for PakSigningKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PakSigningKey")
            .field("key_id", &self.key_id)
            .field("bytes", &"<redacted>")
            .finish()
    }
}

impl PakSigningKey {
    pub fn from_bytes(key_id: impl Into<String>, bytes: [u8; 32]) -> Result<Self, ProtectionError> {
        let key_id = validate_key_id(key_id.into())?;
        Ok(Self { key_id, bytes })
    }

    pub fn from_hex(key_id: impl Into<String>, value: &str) -> Result<Self, ProtectionError> {
        let bytes = decode_fixed_hex::<32>(value, "Ed25519 signing key")?;
        Self::from_bytes(key_id, bytes)
    }

    #[must_use]
    pub fn key_id(&self) -> &str {
        &self.key_id
    }

    #[must_use]
    pub fn verifying_key_bytes(&self) -> [u8; 32] {
        SigningKey::from_bytes(&self.bytes)
            .verifying_key()
            .to_bytes()
    }

    fn sign(&self, message: &[u8]) -> [u8; SIGNATURE_SIZE] {
        SigningKey::from_bytes(&self.bytes).sign(message).to_bytes()
    }
}

/// A 256-bit chunk encryption key. It is only consumed by the adapter layer.
#[derive(Clone)]
pub struct PakEncryptionKey {
    key_id: String,
    bytes: [u8; 32],
}

impl fmt::Debug for PakEncryptionKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PakEncryptionKey")
            .field("key_id", &self.key_id)
            .field("bytes", &"<redacted>")
            .finish()
    }
}

impl PakEncryptionKey {
    pub fn from_bytes(key_id: impl Into<String>, bytes: [u8; 32]) -> Result<Self, ProtectionError> {
        let key_id = validate_key_id(key_id.into())?;
        Ok(Self { key_id, bytes })
    }

    pub fn from_hex(key_id: impl Into<String>, value: &str) -> Result<Self, ProtectionError> {
        let bytes = decode_fixed_hex::<32>(value, "XChaCha20 encryption key")?;
        Self::from_bytes(key_id, bytes)
    }

    #[must_use]
    pub fn key_id(&self) -> &str {
        &self.key_id
    }
}

/// Narrow key lookup needed by a Player to open signed/protected packs.
pub trait PakKeyProvider: fmt::Debug + Send + Sync {
    fn verification_key(&self, key_id: &str) -> Option<[u8; 32]>;
    fn encryption_key(&self, key_id: &str) -> Option<[u8; 32]>;
}

/// Small in-memory key provider useful for a native launcher, Web bootstrap,
/// and deterministic tests. Production Players can implement the same two
/// lookups over their platform-specific key store.
#[derive(Debug, Default, Clone)]
pub struct StaticPakKeyProvider {
    verification_keys: BTreeMap<String, [u8; 32]>,
    encryption_keys: BTreeMap<String, [u8; 32]>,
}

impl StaticPakKeyProvider {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_signing_key(mut self, key: &PakSigningKey) -> Self {
        self.verification_keys
            .insert(key.key_id.clone(), key.verifying_key_bytes());
        self
    }

    pub fn with_verification_key(mut self, key_id: impl Into<String>, key: [u8; 32]) -> Self {
        self.verification_keys.insert(key_id.into(), key);
        self
    }

    pub fn with_encryption_key(mut self, key: &PakEncryptionKey) -> Self {
        self.encryption_keys.insert(key.key_id.clone(), key.bytes);
        self
    }
}

impl PakKeyProvider for StaticPakKeyProvider {
    fn verification_key(&self, key_id: &str) -> Option<[u8; 32]> {
        self.verification_keys.get(key_id).copied()
    }

    fn encryption_key(&self, key_id: &str) -> Option<[u8; 32]> {
        self.encryption_keys.get(key_id).copied()
    }
}

/// Input to the deterministic pack writer.
#[derive(Debug, Clone)]
pub struct PakBuildInput {
    pub pack_id: String,
    pub game_id: String,
    pub role: PakRole,
    pub subtype: String,
    pub dependencies: Vec<PakDependency>,
    pub priority: i32,
    pub assets: Vec<AssetInput>,
    pub profile: PakProfile,
    pub signing_key: Option<PakSigningKey>,
    pub encryption_key: Option<PakEncryptionKey>,
    pub license_policy: LicensePolicy,
}

impl PakBuildInput {
    #[must_use]
    pub fn new(
        pack_id: impl Into<String>,
        game_id: impl Into<String>,
        role: PakRole,
        assets: Vec<AssetInput>,
    ) -> Self {
        Self {
            pack_id: pack_id.into(),
            game_id: game_id.into(),
            role,
            subtype: "base".to_owned(),
            dependencies: Vec::new(),
            priority: 0,
            assets,
            profile: PakProfile::Dev,
            signing_key: None,
            encryption_key: None,
            license_policy: LicensePolicy::none(),
        }
    }
}

/// A parsed role pack. The inner Core archive is only materialized when an
/// asset is requested, so protected chunks remain encrypted at rest.
#[derive(Debug, Clone)]
pub struct PakPackage {
    manifest: PakManifest,
    descriptors: Vec<ChunkDescriptor>,
    manifest_bytes: Vec<u8>,
    payload: Vec<u8>,
    signature: Option<[u8; SIGNATURE_SIZE]>,
    encryption_key: Option<[u8; 32]>,
}

impl PakPackage {
    /// Builds one `.ariapak` pack. Empty roles should simply not be passed to
    /// this function; callers are free to create any number of non-empty
    /// packs and describe their dependencies in each manifest.
    pub fn build(input: PakBuildInput) -> Result<Vec<u8>, ProtectionError> {
        validate_build_input(&input)?;
        let archive = PakArchive::build(input.game_id.clone(), input.assets)?;
        let archive_hash = blake3::hash(&archive).to_hex().to_string();
        let inner = PakArchive::open(&archive)?;
        let content_root = inner.content_root_hex();
        let profile = input.profile;
        let manifest = PakManifest {
            format_version: PAK_FORMAT_VERSION,
            pack_id: input.pack_id,
            game_id: input.game_id,
            role: input.role,
            subtype: input.subtype,
            dependencies: input.dependencies,
            priority: input.priority,
            content_root,
            archive_hash,
            profile,
            signature_key_id: input.signing_key.as_ref().map(|key| key.key_id.clone()),
            encryption_key_id: input.encryption_key.as_ref().map(|key| key.key_id.clone()),
            license_policy: input.license_policy,
        };
        let manifest_bytes = serde_json::to_vec(&manifest)?;
        let mut descriptors = Vec::new();
        let mut payload = Vec::new();
        for (index, raw) in archive.chunks(PAK_CHUNK_MAX_RAW_SIZE).enumerate() {
            let raw_size = u32::try_from(raw.len()).map_err(|_| ProtectionError::TooLarge)?;
            let raw_hash = *blake3::hash(raw).as_bytes();
            let compressed = deflate(raw)?;
            let (encoded, compression) = if compressed.len() < raw.len() {
                (compressed, CODEC_DEFLATE)
            } else {
                (raw.to_vec(), CODEC_STORED)
            };
            let nonce = derive_nonce(&manifest.pack_id, &manifest.archive_hash, index);
            let stored = if let Some(key) = input.encryption_key.as_ref() {
                encrypt_chunk(&encoded, &manifest_bytes, index, nonce, &key.bytes)?
            } else {
                encoded.clone()
            };
            let offset = u64::try_from(payload.len()).map_err(|_| ProtectionError::TooLarge)?;
            let encoded_size =
                u32::try_from(encoded.len()).map_err(|_| ProtectionError::TooLarge)?;
            let stored_size = u32::try_from(stored.len()).map_err(|_| ProtectionError::TooLarge)?;
            payload.extend_from_slice(&stored);
            descriptors.push(ChunkDescriptor {
                raw_size,
                encoded_size,
                stored_size,
                offset,
                compression,
                encrypted: input.encryption_key.is_some(),
                nonce,
                stored_hash: *blake3::hash(&stored).as_bytes(),
                raw_hash,
            });
        }
        let descriptor_bytes = encode_descriptors(&descriptors);
        let signature = input.signing_key.as_ref().map(|signing_key| {
            signing_key.sign(&signature_message(
                profile,
                &manifest_bytes,
                &descriptor_bytes,
                &payload,
            ))
        });
        encode_package(profile, &manifest_bytes, &descriptors, &payload, signature)
    }

    /// Opens and authenticates a package. Signed and protected profiles need
    /// a verification key; protected profiles also need their encryption key.
    pub fn open(
        bytes: &[u8],
        key_provider: Option<&dyn PakKeyProvider>,
    ) -> Result<Self, ProtectionError> {
        let parsed = decode_package(bytes)?;
        let manifest: PakManifest = serde_json::from_slice(&parsed.manifest_bytes)?;
        validate_manifest(&manifest, parsed.profile)?;
        if manifest.content_root != hex::encode(parsed.content_root) {
            return Err(ProtectionError::ManifestMismatch(
                "header content root differs from manifest".to_owned(),
            ));
        }
        let signature = match parsed.signature {
            Some(signature) => {
                let key_id = manifest.signature_key_id.as_deref().ok_or(
                    ProtectionError::MissingVerificationKey("<missing id>".to_owned()),
                )?;
                let provider = key_provider
                    .ok_or_else(|| ProtectionError::MissingVerificationKey(key_id.to_owned()))?;
                let key = provider
                    .verification_key(key_id)
                    .ok_or_else(|| ProtectionError::MissingVerificationKey(key_id.to_owned()))?;
                let verifying_key = VerifyingKey::from_bytes(&key)
                    .map_err(|_| ProtectionError::InvalidVerificationKey(key_id.to_owned()))?;
                verifying_key
                    .verify(
                        &signature_message(
                            parsed.profile,
                            &parsed.manifest_bytes,
                            &parsed.descriptor_bytes,
                            &parsed.payload,
                        ),
                        &Signature::from_bytes(&signature),
                    )
                    .map_err(|_| ProtectionError::SignatureMismatch)?;
                Some(signature)
            }
            None if parsed.profile.is_signed() => {
                return Err(ProtectionError::SignatureMismatch);
            }
            None => None,
        };
        let encryption_key = if parsed.profile.is_encrypted() {
            let key_id = manifest.encryption_key_id.as_deref().ok_or(
                ProtectionError::MissingEncryptionKey("<missing id>".to_owned()),
            )?;
            let provider = key_provider
                .ok_or_else(|| ProtectionError::MissingEncryptionKey(key_id.to_owned()))?;
            Some(
                provider
                    .encryption_key(key_id)
                    .ok_or_else(|| ProtectionError::MissingEncryptionKey(key_id.to_owned()))?,
            )
        } else {
            None
        };
        Ok(Self {
            manifest,
            descriptors: parsed.descriptors,
            manifest_bytes: parsed.manifest_bytes,
            payload: parsed.payload,
            signature,
            encryption_key,
        })
    }

    #[must_use]
    pub fn manifest(&self) -> &PakManifest {
        &self.manifest
    }

    #[must_use]
    pub fn content_root(&self) -> &str {
        &self.manifest.content_root
    }

    #[must_use]
    pub fn content_root_hex(&self) -> &str {
        self.content_root()
    }

    #[must_use]
    pub fn chunk_count(&self) -> usize {
        self.descriptors.len()
    }

    #[must_use]
    pub fn is_signed(&self) -> bool {
        self.signature.is_some()
    }

    #[must_use]
    pub fn is_encrypted(&self) -> bool {
        self.encryption_key.is_some()
    }

    /// Reconstructs the deterministic inner Core archive and verifies its
    /// archive hash and content root before returning it.
    pub fn archive_bytes(&self) -> Result<Vec<u8>, ProtectionError> {
        let mut archive = Vec::new();
        for (index, descriptor) in self.descriptors.iter().enumerate() {
            let start = usize::try_from(descriptor.offset).map_err(|_| ProtectionError::Corrupt)?;
            let end = start
                .checked_add(
                    usize::try_from(descriptor.stored_size)
                        .map_err(|_| ProtectionError::Corrupt)?,
                )
                .ok_or(ProtectionError::Corrupt)?;
            let stored = self
                .payload
                .get(start..end)
                .ok_or(ProtectionError::Corrupt)?;
            if blake3::hash(stored).as_bytes() != &descriptor.stored_hash {
                return Err(ProtectionError::ChunkHashMismatch { chunk: index });
            }
            let encoded = if descriptor.encrypted {
                let key = self.encryption_key.ok_or_else(|| {
                    ProtectionError::MissingEncryptionKey(
                        self.manifest.encryption_key_id.clone().unwrap_or_default(),
                    )
                })?;
                decrypt_chunk(stored, &self.manifest_bytes, index, descriptor.nonce, &key)?
            } else {
                stored.to_vec()
            };
            if encoded.len()
                != usize::try_from(descriptor.encoded_size).map_err(|_| ProtectionError::Corrupt)?
            {
                return Err(ProtectionError::Corrupt);
            }
            let raw = match descriptor.compression {
                CODEC_STORED => {
                    if encoded.len()
                        != usize::try_from(descriptor.raw_size)
                            .map_err(|_| ProtectionError::Corrupt)?
                    {
                        return Err(ProtectionError::Corrupt);
                    }
                    encoded
                }
                CODEC_DEFLATE => inflate(
                    &encoded,
                    usize::try_from(descriptor.raw_size).map_err(|_| ProtectionError::Corrupt)?,
                )?,
                _ => return Err(ProtectionError::Corrupt),
            };
            if blake3::hash(&raw).as_bytes() != &descriptor.raw_hash {
                return Err(ProtectionError::ChunkHashMismatch { chunk: index });
            }
            archive.extend_from_slice(&raw);
        }
        if blake3::hash(&archive).to_hex().as_str() != self.manifest.archive_hash {
            return Err(ProtectionError::ArchiveHashMismatch);
        }
        let inner = PakArchive::open(&archive)?;
        if inner.content_root_hex() != self.manifest.content_root {
            return Err(ProtectionError::ContentRootMismatch);
        }
        Ok(archive)
    }

    /// Reads an asset through the Core archive only after package checks pass.
    pub fn read(&self, logical_path: &str) -> Result<Vec<u8>, ProtectionError> {
        Ok(PakArchive::open(&self.archive_bytes()?)?.read(logical_path)?)
    }

    /// Authorizes a protected pack at an explicit Player-provided time. A
    /// valid cached/provider lease is used without renewal; an absent or
    /// invalid lease causes exactly one renewal request after entitlement has
    /// been confirmed. The Core VM never calls this method.
    pub fn authorize(
        &self,
        provider: &dyn LicenseProvider,
        key_provider: &dyn PakKeyProvider,
        now_unix_ms: u64,
        current_lease: Option<&LicenseLease>,
    ) -> Result<LicenseAuthorization, LicenseError> {
        authorize_manifest(
            &self.manifest,
            provider,
            key_provider,
            now_unix_ms,
            current_lease,
        )
    }
}

#[derive(Debug, Clone)]
struct ChunkDescriptor {
    raw_size: u32,
    encoded_size: u32,
    stored_size: u32,
    offset: u64,
    compression: u8,
    encrypted: bool,
    nonce: [u8; 24],
    stored_hash: [u8; 32],
    raw_hash: [u8; 32],
}

#[derive(Debug)]
struct DecodedPackage {
    profile: PakProfile,
    content_root: [u8; 32],
    manifest_bytes: Vec<u8>,
    descriptors: Vec<ChunkDescriptor>,
    descriptor_bytes: Vec<u8>,
    payload: Vec<u8>,
    signature: Option<[u8; SIGNATURE_SIZE]>,
}

fn validate_build_input(input: &PakBuildInput) -> Result<(), ProtectionError> {
    validate_id(&input.pack_id, "pack ID")?;
    validate_id(&input.game_id, "game ID")?;
    if input.subtype.trim().is_empty() || input.subtype.contains('\0') {
        return Err(ProtectionError::InvalidManifest(
            "pack subtype must be non-empty and contain no NUL".to_owned(),
        ));
    }
    if input.profile.is_signed() && input.signing_key.is_none() {
        return Err(ProtectionError::MissingSigningKey);
    }
    if !input.profile.is_signed() && input.signing_key.is_some() {
        return Err(ProtectionError::UnexpectedSigningKey);
    }
    if input.profile.is_encrypted() && input.encryption_key.is_none() {
        return Err(ProtectionError::MissingEncryptionKey("<build>".to_owned()));
    }
    if !input.profile.is_encrypted() && input.encryption_key.is_some() {
        return Err(ProtectionError::UnexpectedEncryptionKey);
    }
    if input.profile.is_encrypted()
        && (!input.license_policy.required || !input.license_policy.offline_allowed)
    {
        return Err(ProtectionError::InvalidManifest(
            "protected packs require an offline-capable license policy".to_owned(),
        ));
    }
    if input.license_policy.required && input.license_policy.lease_seconds == 0 {
        return Err(ProtectionError::InvalidManifest(
            "a required license policy must declare a non-zero lease".to_owned(),
        ));
    }
    Ok(())
}

fn validate_manifest(manifest: &PakManifest, profile: PakProfile) -> Result<(), ProtectionError> {
    if manifest.format_version != PAK_FORMAT_VERSION {
        return Err(ProtectionError::UnsupportedVersion(manifest.format_version));
    }
    if manifest.profile != profile {
        return Err(ProtectionError::ManifestMismatch(
            "manifest profile differs from package header".to_owned(),
        ));
    }
    validate_id(&manifest.pack_id, "pack ID")?;
    validate_id(&manifest.game_id, "game ID")?;
    if profile.is_signed() != manifest.signature_key_id.is_some() {
        return Err(ProtectionError::ManifestMismatch(
            "signature key ID does not match profile".to_owned(),
        ));
    }
    if profile.is_encrypted() != manifest.encryption_key_id.is_some() {
        return Err(ProtectionError::ManifestMismatch(
            "encryption key ID does not match profile".to_owned(),
        ));
    }
    if profile.requires_encryption()
        && (!manifest.license_policy.required || !manifest.license_policy.offline_allowed)
    {
        return Err(ProtectionError::InvalidManifest(
            "protected packs require an offline-capable license policy".to_owned(),
        ));
    }
    Ok(())
}

fn encode_package(
    profile: PakProfile,
    manifest: &[u8],
    descriptors: &[ChunkDescriptor],
    payload: &[u8],
    signature: Option<[u8; SIGNATURE_SIZE]>,
) -> Result<Vec<u8>, ProtectionError> {
    let descriptor_bytes = encode_descriptors(descriptors);
    let manifest_len = u32::try_from(manifest.len()).map_err(|_| ProtectionError::TooLarge)?;
    let descriptor_len =
        u32::try_from(descriptor_bytes.len()).map_err(|_| ProtectionError::TooLarge)?;
    let chunk_count = u32::try_from(descriptors.len()).map_err(|_| ProtectionError::TooLarge)?;
    let payload_len = u64::try_from(payload.len()).map_err(|_| ProtectionError::TooLarge)?;
    let root = hex::decode(serde_json::from_slice::<PakManifest>(manifest)?.content_root)
        .map_err(|_| ProtectionError::InvalidManifest("content root is not hex".to_owned()))?;
    let content_root: [u8; 32] = root
        .try_into()
        .map_err(|_| ProtectionError::InvalidManifest("content root must be BLAKE3".to_owned()))?;
    let signature_len = u16::try_from(signature.map_or(0, |_| SIGNATURE_SIZE))
        .map_err(|_| ProtectionError::TooLarge)?;
    let total = HEADER_SIZE
        .checked_add(manifest.len())
        .and_then(|value| value.checked_add(descriptor_bytes.len()))
        .and_then(|value| value.checked_add(payload.len()))
        .and_then(|value| value.checked_add(usize::from(signature_len)))
        .ok_or(ProtectionError::TooLarge)?;
    let mut output = Vec::with_capacity(total);
    output.extend_from_slice(&PROTECTED_PAK_MAGIC);
    output.extend_from_slice(&PAK_FORMAT_VERSION.to_le_bytes());
    output.push(profile as u8);
    output.push(FLAG_NONE);
    output.extend_from_slice(&manifest_len.to_le_bytes());
    output.extend_from_slice(&chunk_count.to_le_bytes());
    output.extend_from_slice(&descriptor_len.to_le_bytes());
    output.extend_from_slice(&signature_len.to_le_bytes());
    output.extend_from_slice(&0_u16.to_le_bytes());
    output.extend_from_slice(&payload_len.to_le_bytes());
    output.extend_from_slice(&content_root);
    output.extend_from_slice(&[0; 4]);
    debug_assert_eq!(output.len(), HEADER_SIZE);
    output.extend_from_slice(manifest);
    output.extend_from_slice(&descriptor_bytes);
    output.extend_from_slice(payload);
    if let Some(signature) = signature {
        output.extend_from_slice(&signature);
    }
    Ok(output)
}

fn decode_package(bytes: &[u8]) -> Result<DecodedPackage, ProtectionError> {
    if bytes.len() < HEADER_SIZE {
        return Err(ProtectionError::Truncated);
    }
    let mut cursor = 0;
    if take::<8>(bytes, &mut cursor)? != PROTECTED_PAK_MAGIC {
        return Err(ProtectionError::InvalidMagic);
    }
    let version = take_u16(bytes, &mut cursor)?;
    if version != PAK_FORMAT_VERSION {
        return Err(ProtectionError::UnsupportedVersion(version));
    }
    let profile = profile_from_byte(take::<1>(bytes, &mut cursor)?[0])?;
    if take::<1>(bytes, &mut cursor)?[0] != FLAG_NONE {
        return Err(ProtectionError::InvalidHeader);
    }
    let manifest_len =
        usize::try_from(take_u32(bytes, &mut cursor)?).map_err(|_| ProtectionError::TooLarge)?;
    let chunk_count =
        usize::try_from(take_u32(bytes, &mut cursor)?).map_err(|_| ProtectionError::TooLarge)?;
    let descriptor_len =
        usize::try_from(take_u32(bytes, &mut cursor)?).map_err(|_| ProtectionError::TooLarge)?;
    let signature_len = usize::from(take_u16(bytes, &mut cursor)?);
    if take_u16(bytes, &mut cursor)? != 0 {
        return Err(ProtectionError::InvalidHeader);
    }
    let payload_len =
        usize::try_from(take_u64(bytes, &mut cursor)?).map_err(|_| ProtectionError::TooLarge)?;
    let content_root = take::<32>(bytes, &mut cursor)?;
    if take::<4>(bytes, &mut cursor)? != [0; 4]
        || descriptor_len
            != chunk_count
                .checked_mul(DESCRIPTOR_SIZE)
                .ok_or(ProtectionError::TooLarge)?
        || (profile.is_signed() && signature_len != SIGNATURE_SIZE)
        || (!profile.is_signed() && signature_len != 0)
    {
        return Err(ProtectionError::InvalidHeader);
    }
    let manifest_bytes = take_slice(bytes, &mut cursor, manifest_len)?.to_vec();
    let descriptor_bytes = take_slice(bytes, &mut cursor, descriptor_len)?.to_vec();
    let payload = take_slice(bytes, &mut cursor, payload_len)?.to_vec();
    let signature = if signature_len == SIGNATURE_SIZE {
        Some(take::<SIGNATURE_SIZE>(bytes, &mut cursor)?)
    } else {
        None
    };
    if cursor != bytes.len() {
        return Err(ProtectionError::Truncated);
    }
    let descriptors = decode_descriptors(&descriptor_bytes, chunk_count, payload.len(), profile)?;
    Ok(DecodedPackage {
        profile,
        content_root,
        manifest_bytes,
        descriptors,
        descriptor_bytes,
        payload,
        signature,
    })
}

fn encode_descriptors(descriptors: &[ChunkDescriptor]) -> Vec<u8> {
    let mut output = Vec::with_capacity(descriptors.len() * DESCRIPTOR_SIZE);
    for descriptor in descriptors {
        output.extend_from_slice(&descriptor.raw_size.to_le_bytes());
        output.extend_from_slice(&descriptor.encoded_size.to_le_bytes());
        output.extend_from_slice(&descriptor.stored_size.to_le_bytes());
        output.extend_from_slice(&descriptor.offset.to_le_bytes());
        output.push(descriptor.compression);
        output.push(u8::from(descriptor.encrypted));
        output.extend_from_slice(&[0; 2]);
        output.extend_from_slice(&descriptor.nonce);
        output.extend_from_slice(&descriptor.stored_hash);
        output.extend_from_slice(&descriptor.raw_hash);
    }
    output
}

fn decode_descriptors(
    bytes: &[u8],
    count: usize,
    payload_len: usize,
    profile: PakProfile,
) -> Result<Vec<ChunkDescriptor>, ProtectionError> {
    if bytes.len()
        != count
            .checked_mul(DESCRIPTOR_SIZE)
            .ok_or(ProtectionError::TooLarge)?
    {
        return Err(ProtectionError::InvalidHeader);
    }
    let mut cursor = 0;
    let mut output = Vec::with_capacity(count);
    let mut expected_offset = 0_u64;
    for _ in 0..count {
        let raw_size = take_u32(bytes, &mut cursor)?;
        let encoded_size = take_u32(bytes, &mut cursor)?;
        let stored_size = take_u32(bytes, &mut cursor)?;
        let offset = take_u64(bytes, &mut cursor)?;
        let compression = take::<1>(bytes, &mut cursor)?[0];
        let encrypted = take::<1>(bytes, &mut cursor)?[0];
        if take::<2>(bytes, &mut cursor)? != [0; 2]
            || raw_size == 0
            || usize::try_from(raw_size).unwrap_or(usize::MAX) > PAK_CHUNK_MAX_RAW_SIZE
            || encoded_size == 0
            || stored_size == 0
            || !matches!(compression, CODEC_STORED | CODEC_DEFLATE)
            || encrypted > 1
            || (profile.is_encrypted() != (encrypted != 0))
            || (!profile.is_encrypted() && encrypted != 0)
            || offset != expected_offset
        {
            return Err(ProtectionError::InvalidDescriptor);
        }
        let nonce = take::<24>(bytes, &mut cursor)?;
        let stored_hash = take::<32>(bytes, &mut cursor)?;
        let raw_hash = take::<32>(bytes, &mut cursor)?;
        let end = usize::try_from(offset)
            .ok()
            .and_then(|value| value.checked_add(usize::try_from(stored_size).ok()?))
            .ok_or(ProtectionError::TooLarge)?;
        if end > payload_len {
            return Err(ProtectionError::InvalidDescriptor);
        }
        if profile.is_encrypted() {
            if stored_size != encoded_size.saturating_add(16) {
                return Err(ProtectionError::InvalidDescriptor);
            }
        } else if stored_size != encoded_size {
            return Err(ProtectionError::InvalidDescriptor);
        }
        expected_offset = offset
            .checked_add(u64::from(stored_size))
            .ok_or(ProtectionError::TooLarge)?;
        output.push(ChunkDescriptor {
            raw_size,
            encoded_size,
            stored_size,
            offset,
            compression,
            encrypted: encrypted != 0,
            nonce,
            stored_hash,
            raw_hash,
        });
    }
    if usize::try_from(expected_offset).map_err(|_| ProtectionError::TooLarge)? != payload_len {
        return Err(ProtectionError::InvalidDescriptor);
    }
    Ok(output)
}

fn signature_message(
    profile: PakProfile,
    manifest: &[u8],
    descriptors: &[u8],
    payload: &[u8],
) -> Vec<u8> {
    let mut output = Vec::with_capacity(16 + manifest.len() + descriptors.len() + payload.len());
    output.extend_from_slice(b"AriaEngine PAK4 signature\0");
    output.push(profile as u8);
    output.extend_from_slice(&(manifest.len() as u64).to_le_bytes());
    output.extend_from_slice(manifest);
    output.extend_from_slice(&(descriptors.len() as u64).to_le_bytes());
    output.extend_from_slice(descriptors);
    output.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    output.extend_from_slice(payload);
    output
}

fn derive_nonce(pack_id: &str, archive_hash: &str, chunk: usize) -> [u8; 24] {
    let mut hasher = blake3::Hasher::new_derive_key("AriaEngine PAK4 XChaCha nonce");
    hasher.update(&(pack_id.len() as u64).to_le_bytes());
    hasher.update(pack_id.as_bytes());
    hasher.update(&(archive_hash.len() as u64).to_le_bytes());
    hasher.update(archive_hash.as_bytes());
    hasher.update(&(chunk as u64).to_le_bytes());
    let hash = hasher.finalize();
    let mut nonce = [0_u8; 24];
    nonce.copy_from_slice(&hash.as_bytes()[..24]);
    nonce
}

fn encrypt_chunk(
    plaintext: &[u8],
    manifest: &[u8],
    index: usize,
    nonce: [u8; 24],
    key: &[u8; 32],
) -> Result<Vec<u8>, ProtectionError> {
    let cipher = XChaCha20Poly1305::new_from_slice(key)
        .map_err(|_| ProtectionError::InvalidEncryptionKey)?;
    let mut buffer = plaintext.to_vec();
    let tag = cipher
        .encrypt_in_place_detached(
            XNonce::from_slice(&nonce),
            &associated_data(manifest, index),
            &mut buffer,
        )
        .map_err(|_| ProtectionError::EncryptionFailed)?;
    buffer.extend_from_slice(&tag);
    Ok(buffer)
}

fn decrypt_chunk(
    stored: &[u8],
    manifest: &[u8],
    index: usize,
    nonce: [u8; 24],
    key: &[u8; 32],
) -> Result<Vec<u8>, ProtectionError> {
    if stored.len() < 16 {
        return Err(ProtectionError::DecryptionFailed);
    }
    let cipher = XChaCha20Poly1305::new_from_slice(key)
        .map_err(|_| ProtectionError::InvalidEncryptionKey)?;
    let split = stored.len() - 16;
    let (ciphertext, tag) = stored.split_at(split);
    let mut buffer = ciphertext.to_vec();
    let tag = Tag::from_slice(tag);
    cipher
        .decrypt_in_place_detached(
            XNonce::from_slice(&nonce),
            &associated_data(manifest, index),
            &mut buffer,
            tag,
        )
        .map_err(|_| ProtectionError::DecryptionFailed)?;
    Ok(buffer)
}

fn associated_data(manifest: &[u8], index: usize) -> Vec<u8> {
    let mut data = Vec::with_capacity(manifest.len() + 8);
    data.extend_from_slice(manifest);
    data.extend_from_slice(&(index as u64).to_le_bytes());
    data
}

fn deflate(bytes: &[u8]) -> Result<Vec<u8>, ProtectionError> {
    let mut encoder = DeflateEncoder::new(Vec::new(), Compression::new(9));
    encoder
        .write_all(bytes)
        .map_err(ProtectionError::Compression)?;
    encoder.finish().map_err(ProtectionError::Compression)
}

fn inflate(bytes: &[u8], expected: usize) -> Result<Vec<u8>, ProtectionError> {
    let mut output = Vec::with_capacity(expected);
    DeflateDecoder::new(bytes)
        .take(
            u64::try_from(expected)
                .unwrap_or(u64::MAX)
                .saturating_add(1),
        )
        .read_to_end(&mut output)
        .map_err(ProtectionError::Compression)?;
    if output.len() != expected {
        return Err(ProtectionError::InvalidCompressedSize {
            expected,
            actual: output.len(),
        });
    }
    Ok(output)
}

fn profile_from_byte(value: u8) -> Result<PakProfile, ProtectionError> {
    match value {
        0 => Ok(PakProfile::Dev),
        1 => Ok(PakProfile::Signed),
        2 => Ok(PakProfile::Protected),
        _ => Err(ProtectionError::InvalidHeader),
    }
}

fn validate_id(value: &str, label: &str) -> Result<(), ProtectionError> {
    if value.trim().is_empty() || value.contains('\0') {
        return Err(ProtectionError::InvalidManifest(format!(
            "{label} must be non-empty and contain no NUL"
        )));
    }
    Ok(())
}

fn validate_key_id(value: String) -> Result<String, ProtectionError> {
    validate_id(&value, "key ID")?;
    Ok(value)
}

fn decode_fixed_hex<const N: usize>(value: &str, label: &str) -> Result<[u8; N], ProtectionError> {
    let decoded = hex::decode(value).map_err(|_| ProtectionError::InvalidKey(label.to_owned()))?;
    decoded
        .try_into()
        .map_err(|_| ProtectionError::InvalidKey(label.to_owned()))
}

fn take_slice<'a>(
    bytes: &'a [u8],
    cursor: &mut usize,
    length: usize,
) -> Result<&'a [u8], ProtectionError> {
    let end = cursor
        .checked_add(length)
        .ok_or(ProtectionError::Truncated)?;
    let slice = bytes.get(*cursor..end).ok_or(ProtectionError::Truncated)?;
    *cursor = end;
    Ok(slice)
}

fn take<const N: usize>(bytes: &[u8], cursor: &mut usize) -> Result<[u8; N], ProtectionError> {
    take_slice(bytes, cursor, N)?
        .try_into()
        .map_err(|_| ProtectionError::Truncated)
}

fn take_u16(bytes: &[u8], cursor: &mut usize) -> Result<u16, ProtectionError> {
    Ok(u16::from_le_bytes(take(bytes, cursor)?))
}

fn take_u32(bytes: &[u8], cursor: &mut usize) -> Result<u32, ProtectionError> {
    Ok(u32::from_le_bytes(take(bytes, cursor)?))
}

fn take_u64(bytes: &[u8], cursor: &mut usize) -> Result<u64, ProtectionError> {
    Ok(u64::from_le_bytes(take(bytes, cursor)?))
}

/// Errors raised by package authentication, decryption, or lease handling.
#[derive(Debug, Error)]
pub enum ProtectionError {
    #[error("invalid PAK4 protected magic")]
    InvalidMagic,
    #[error("truncated PAK4 package")]
    Truncated,
    #[error("unsupported PAK4 package version {0}")]
    UnsupportedVersion(u16),
    #[error("invalid PAK4 package header")]
    InvalidHeader,
    #[error("invalid PAK4 chunk descriptor")]
    InvalidDescriptor,
    #[error("package payload is internally inconsistent")]
    Corrupt,
    #[error("invalid package manifest: {0}")]
    InvalidManifest(String),
    #[error("duplicate pack ID '{0}'")]
    DuplicatePackId(String),
    #[error("pack '{pack_id}' depends on missing pack '{dependency}'")]
    MissingDependency { pack_id: String, dependency: String },
    #[error("pack dependency cycle: {0:?}")]
    DependencyCycle(Vec<String>),
    #[error("manifest does not match package: {0}")]
    ManifestMismatch(String),
    #[error("package is too large")]
    TooLarge,
    #[error("package signature does not verify")]
    SignatureMismatch,
    #[error("verification key '{0}' is not available")]
    MissingVerificationKey(String),
    #[error("verification key '{0}' is invalid")]
    InvalidVerificationKey(String),
    #[error("signing key is required by the selected profile")]
    MissingSigningKey,
    #[error("signing key is not valid for the selected profile")]
    UnexpectedSigningKey,
    #[error("encryption key '{0}' is not available")]
    MissingEncryptionKey(String),
    #[error("encryption key is not valid")]
    InvalidEncryptionKey,
    #[error("encryption key is not valid for the selected profile")]
    UnexpectedEncryptionKey,
    #[error("chunk encryption failed")]
    EncryptionFailed,
    #[error("chunk decryption failed")]
    DecryptionFailed,
    #[error("chunk {chunk} hash does not verify")]
    ChunkHashMismatch { chunk: usize },
    #[error("inner archive hash does not verify")]
    ArchiveHashMismatch,
    #[error("inner archive content root does not verify")]
    ContentRootMismatch,
    #[error("invalid compressed chunk size: expected {expected}, got {actual}")]
    InvalidCompressedSize { expected: usize, actual: usize },
    #[error("invalid key: {0}")]
    InvalidKey(String),
    #[error("compression failed: {0}")]
    Compression(std::io::Error),
    #[error("inner PAK archive failed: {0}")]
    Inner(#[from] PakError),
    #[error("JSON encoding failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("license error: {0}")]
    License(#[from] LicenseError),
}

/// A lease is a signed, portable entitlement assertion. The Player supplies
/// the current time; this type never reads a system clock.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LicenseLease {
    pub format_version: u16,
    pub game_id: String,
    pub pack_id: String,
    pub issued_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    pub grace_until_unix_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revocation_reason: Option<String>,
    pub signature_key_id: String,
    pub signature: String,
}

impl LicenseLease {
    pub const FORMAT_VERSION: u16 = 1;

    pub fn issue(
        key: &PakSigningKey,
        game_id: impl Into<String>,
        pack_id: impl Into<String>,
        issued_at_unix_ms: u64,
        policy: &LicensePolicy,
    ) -> Result<Self, LicenseError> {
        if !policy.required || !policy.offline_allowed || policy.lease_seconds == 0 {
            return Err(LicenseError::InvalidPolicy);
        }
        let game_id = game_id.into();
        let pack_id = pack_id.into();
        if game_id.trim().is_empty()
            || pack_id.trim().is_empty()
            || game_id.contains('\0')
            || pack_id.contains('\0')
        {
            return Err(LicenseError::InvalidLease);
        }
        let lease_millis = policy
            .lease_seconds
            .checked_mul(1000)
            .ok_or(LicenseError::InvalidWindow)?;
        let grace_millis = policy
            .grace_seconds
            .checked_mul(1000)
            .ok_or(LicenseError::InvalidWindow)?;
        let expires_at_unix_ms = issued_at_unix_ms
            .checked_add(lease_millis)
            .ok_or(LicenseError::InvalidWindow)?;
        let grace_until_unix_ms = expires_at_unix_ms
            .checked_add(grace_millis)
            .ok_or(LicenseError::InvalidWindow)?;
        let mut lease = Self {
            format_version: Self::FORMAT_VERSION,
            game_id,
            pack_id,
            issued_at_unix_ms,
            expires_at_unix_ms,
            grace_until_unix_ms,
            revocation_reason: None,
            signature_key_id: key.key_id().to_owned(),
            signature: String::new(),
        };
        lease.signature = hex::encode(key.sign(&lease.unsigned_bytes()?));
        Ok(lease)
    }

    pub fn verify(
        &self,
        verification_key: [u8; 32],
        expected_game_id: &str,
        expected_pack_id: &str,
        policy: &LicensePolicy,
        now_unix_ms: u64,
    ) -> Result<LeaseStatus, LicenseError> {
        if !policy.required || !policy.offline_allowed || policy.lease_seconds == 0 {
            return Err(LicenseError::InvalidPolicy);
        }
        if self.format_version != Self::FORMAT_VERSION
            || self.game_id != expected_game_id
            || self.pack_id != expected_pack_id
            || self.expires_at_unix_ms < self.issued_at_unix_ms
            || self.grace_until_unix_ms < self.expires_at_unix_ms
        {
            return Err(LicenseError::InvalidLease);
        }
        if now_unix_ms < self.issued_at_unix_ms {
            return Err(LicenseError::InvalidWindow);
        }
        if let Some(reason) = &self.revocation_reason {
            return Err(LicenseError::Revoked(reason.clone()));
        }
        let key = VerifyingKey::from_bytes(&verification_key)
            .map_err(|_| LicenseError::InvalidVerificationKey)?;
        let signature_bytes =
            decode_fixed_hex::<SIGNATURE_SIZE>(&self.signature, "lease signature")
                .map_err(|_| LicenseError::InvalidSignature)?;
        key.verify(
            &self.unsigned_bytes()?,
            &Signature::from_bytes(&signature_bytes),
        )
        .map_err(|_| LicenseError::InvalidSignature)?;
        let max_grace = self
            .issued_at_unix_ms
            .checked_add(
                policy
                    .lease_seconds
                    .checked_add(policy.grace_seconds)
                    .ok_or(LicenseError::InvalidWindow)?
                    .checked_mul(1000)
                    .ok_or(LicenseError::InvalidWindow)?,
            )
            .ok_or(LicenseError::InvalidWindow)?;
        let expected_expires = self
            .issued_at_unix_ms
            .checked_add(
                policy
                    .lease_seconds
                    .checked_mul(1000)
                    .ok_or(LicenseError::InvalidWindow)?,
            )
            .ok_or(LicenseError::InvalidWindow)?;
        let expected_grace = expected_expires
            .checked_add(
                policy
                    .grace_seconds
                    .checked_mul(1000)
                    .ok_or(LicenseError::InvalidWindow)?,
            )
            .ok_or(LicenseError::InvalidWindow)?;
        if self.expires_at_unix_ms != expected_expires
            || self.grace_until_unix_ms != expected_grace
            || self.grace_until_unix_ms > max_grace
        {
            return Err(LicenseError::InvalidWindow);
        }
        if now_unix_ms <= self.expires_at_unix_ms {
            Ok(LeaseStatus {
                within_grace: false,
            })
        } else if policy.offline_allowed && now_unix_ms <= self.grace_until_unix_ms {
            Ok(LeaseStatus { within_grace: true })
        } else {
            Err(LicenseError::Expired)
        }
    }

    fn unsigned_bytes(&self) -> Result<Vec<u8>, LicenseError> {
        serde_json::to_vec(&UnsignedLease {
            format_version: self.format_version,
            game_id: &self.game_id,
            pack_id: &self.pack_id,
            issued_at_unix_ms: self.issued_at_unix_ms,
            expires_at_unix_ms: self.expires_at_unix_ms,
            grace_until_unix_ms: self.grace_until_unix_ms,
            revocation_reason: self.revocation_reason.as_deref(),
            signature_key_id: &self.signature_key_id,
        })
        .map_err(|_| LicenseError::Encoding)
    }
}

#[derive(Debug, Serialize)]
struct UnsignedLease<'a> {
    format_version: u16,
    game_id: &'a str,
    pack_id: &'a str,
    issued_at_unix_ms: u64,
    expires_at_unix_ms: u64,
    grace_until_unix_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    revocation_reason: Option<&'a str>,
    signature_key_id: &'a str,
}

/// Result of lease validation; grace usage can be surfaced in diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LeaseStatus {
    pub within_grace: bool,
}

/// The result a Player retains for the duration of a protected-pack session.
/// `within_grace` is deliberately explicit so it can be written to a
/// diagnostic log without exposing provider or key internals.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LicenseAuthorization {
    pub lease: LicenseLease,
    pub status: LeaseStatus,
}

/// Data passed to the Player's entitlement check. Time is an explicit input
/// so replay and Web/native behavior remain deterministic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntitlementRequest {
    pub game_id: String,
    pub pack_id: String,
    pub now_unix_ms: u64,
}

/// Result returned by an entitlement provider.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entitlement {
    pub entitled: bool,
    pub lease: Option<LicenseLease>,
    pub reason: Option<String>,
}

/// Data passed to lease renewal. The provider decides how to contact its
/// service; no network operation is part of the Player ABI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeaseRequest {
    pub game_id: String,
    pub pack_id: String,
    pub policy: LicensePolicy,
    pub now_unix_ms: u64,
    pub current_lease: Option<LicenseLease>,
}

/// The complete Player-facing license ABI: entitlement check and lease renew.
pub trait LicenseProvider: fmt::Debug + Send + Sync {
    fn check_entitlement(&self, request: &EntitlementRequest) -> Result<Entitlement, LicenseError>;

    fn renew_lease(&self, request: &LeaseRequest) -> Result<LicenseLease, LicenseError>;
}

fn authorize_manifest(
    manifest: &PakManifest,
    provider: &dyn LicenseProvider,
    key_provider: &dyn PakKeyProvider,
    now_unix_ms: u64,
    current_lease: Option<&LicenseLease>,
) -> Result<LicenseAuthorization, LicenseError> {
    if manifest.profile != PakProfile::Protected {
        return Err(LicenseError::InvalidPolicy);
    }
    let request = EntitlementRequest {
        game_id: manifest.game_id.clone(),
        pack_id: manifest.pack_id.clone(),
        now_unix_ms,
    };
    let entitlement = provider.check_entitlement(&request)?;
    if !entitlement.entitled {
        return Err(LicenseError::NotEntitled);
    }

    let candidate = entitlement.lease.clone().or_else(|| current_lease.cloned());
    if let Some(lease) = candidate.as_ref()
        && let Ok(authorization) = verify_manifest_lease(manifest, key_provider, lease, now_unix_ms)
    {
        return Ok(authorization);
    }

    let renewed = provider.renew_lease(&LeaseRequest {
        game_id: manifest.game_id.clone(),
        pack_id: manifest.pack_id.clone(),
        policy: manifest.license_policy.clone(),
        now_unix_ms,
        current_lease: candidate,
    })?;
    verify_manifest_lease(manifest, key_provider, &renewed, now_unix_ms)
}

fn verify_manifest_lease(
    manifest: &PakManifest,
    key_provider: &dyn PakKeyProvider,
    lease: &LicenseLease,
    now_unix_ms: u64,
) -> Result<LicenseAuthorization, LicenseError> {
    let verification_key = key_provider
        .verification_key(&lease.signature_key_id)
        .ok_or_else(|| LicenseError::MissingVerificationKey(lease.signature_key_id.clone()))?;
    let status = lease.verify(
        verification_key,
        &manifest.game_id,
        &manifest.pack_id,
        &manifest.license_policy,
        now_unix_ms,
    )?;
    Ok(LicenseAuthorization {
        lease: lease.clone(),
        status,
    })
}

/// Namespace alias for callers that want to keep package concerns separate
/// from entitlement concerns without adding a plugin ABI.
pub mod pak {
    pub use super::{
        PROTECTED_PAK_MAGIC, PakBuildInput, PakDependency, PakEncryptionKey, PakKeyProvider,
        PakManifest, PakPackage, PakProfile, PakRole, PakSigningKey, StaticPakKeyProvider,
        validate_pack_set,
    };
}

/// Namespace alias for the Player-facing entitlement contract.
pub mod license {
    pub use super::{
        Entitlement, EntitlementRequest, LeaseRequest, LeaseStatus, LicenseAuthorization,
        LicenseError, LicenseLease, LicensePolicy, LicenseProvider,
    };
}

/// License failures are deliberately about entitlement state, not transport
/// details. Providers may map HTTP, OS, or browser errors into these values.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum LicenseError {
    #[error("license policy does not allow an offline lease")]
    InvalidPolicy,
    #[error("license lease has an invalid time window")]
    InvalidWindow,
    #[error("license lease is malformed")]
    InvalidLease,
    #[error("license lease signature is invalid")]
    InvalidSignature,
    #[error("license verification key is invalid")]
    InvalidVerificationKey,
    #[error("license verification key '{0}' is not available")]
    MissingVerificationKey(String),
    #[error("license lease is expired")]
    Expired,
    #[error("license lease was revoked: {0}")]
    Revoked(String),
    #[error("entitlement is not available")]
    NotEntitled,
    #[error("license payload encoding failed")]
    Encoding,
    #[error("license provider failed: {0}")]
    Provider(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signing_key() -> PakSigningKey {
        PakSigningKey::from_bytes("publisher", [7; 32]).unwrap()
    }

    fn encryption_key() -> PakEncryptionKey {
        PakEncryptionKey::from_bytes("content", [9; 32]).unwrap()
    }

    fn assets() -> Vec<AssetInput> {
        vec![AssetInput {
            logical_path: "assets/scene.txt".to_owned(),
            bytes: b"same content on every Player".to_vec(),
        }]
    }

    #[test]
    fn dev_package_is_deterministic_and_reads_through_core() {
        let input = PakBuildInput::new("game.boot", "jp.example.game", PakRole::Boot, assets());
        let first = PakPackage::build(input.clone()).unwrap();
        let second = PakPackage::build(input).unwrap();
        assert_eq!(first, second);
        let package = PakPackage::open(&first, None).unwrap();
        assert_eq!(package.manifest().profile, PakProfile::Dev);
        assert_eq!(
            package.read("assets/scene.txt").unwrap(),
            b"same content on every Player"
        );
    }

    #[test]
    fn signed_and_protected_profiles_verify_and_reject_tampering() {
        let signer = signing_key();
        let signed = PakPackage::build(PakBuildInput {
            profile: PakProfile::Signed,
            signing_key: Some(signer.clone()),
            ..PakBuildInput::new("game.hot", "jp.example.game", PakRole::Hot, assets())
        })
        .unwrap();
        let keys = StaticPakKeyProvider::new().with_signing_key(&signer);
        let package = PakPackage::open(&signed, Some(&keys)).unwrap();
        assert!(package.is_signed());
        assert_eq!(
            package.read("assets/scene.txt").unwrap(),
            b"same content on every Player"
        );

        let mut tampered = signed.clone();
        tampered[HEADER_SIZE + 1] ^= 1;
        assert!(matches!(
            PakPackage::open(&tampered, Some(&keys)),
            Err(ProtectionError::SignatureMismatch | ProtectionError::Json(_))
        ));

        let encryption = encryption_key();
        let protected = PakPackage::build(PakBuildInput {
            profile: PakProfile::Protected,
            signing_key: Some(signer.clone()),
            encryption_key: Some(encryption.clone()),
            license_policy: LicensePolicy::offline(3600, 300),
            ..PakBuildInput::new("game.cold", "jp.example.game", PakRole::Cold, assets())
        })
        .unwrap();
        let protected_keys = StaticPakKeyProvider::new()
            .with_signing_key(&signer)
            .with_encryption_key(&encryption);
        let package = PakPackage::open(&protected, Some(&protected_keys)).unwrap();
        assert!(package.is_encrypted());
        assert_eq!(
            package.read("assets/scene.txt").unwrap(),
            b"same content on every Player"
        );
    }

    #[test]
    fn lease_is_offline_verifiable_and_exposes_grace() {
        let signer = signing_key();
        let policy = LicensePolicy::offline(60, 30);
        let lease =
            LicenseLease::issue(&signer, "jp.example.game", "game.cold", 1_000, &policy).unwrap();
        let status = lease
            .verify(
                signer.verifying_key_bytes(),
                "jp.example.game",
                "game.cold",
                &policy,
                61_001,
            )
            .unwrap();
        assert!(status.within_grace);
        assert!(matches!(
            lease.verify(
                signer.verifying_key_bytes(),
                "jp.example.game",
                "game.cold",
                &policy,
                91_001,
            ),
            Err(LicenseError::Expired)
        ));
    }

    #[derive(Debug)]
    struct TestLicenseProvider {
        entitlement_lease: Option<LicenseLease>,
        renewal: LicenseLease,
    }

    impl LicenseProvider for TestLicenseProvider {
        fn check_entitlement(
            &self,
            _request: &EntitlementRequest,
        ) -> Result<Entitlement, LicenseError> {
            Ok(Entitlement {
                entitled: true,
                lease: self.entitlement_lease.clone(),
                reason: None,
            })
        }

        fn renew_lease(&self, _request: &LeaseRequest) -> Result<LicenseLease, LicenseError> {
            Ok(self.renewal.clone())
        }
    }

    #[test]
    fn protected_package_authorization_uses_offline_lease_then_renews() {
        let signer = signing_key();
        let encryption = encryption_key();
        let policy = LicensePolicy::offline(60, 30);
        let package_bytes = PakPackage::build(PakBuildInput {
            profile: PakProfile::Protected,
            signing_key: Some(signer.clone()),
            encryption_key: Some(encryption.clone()),
            license_policy: policy.clone(),
            ..PakBuildInput::new("game.cold", "jp.example.game", PakRole::Cold, assets())
        })
        .unwrap();
        let keys = StaticPakKeyProvider::new()
            .with_signing_key(&signer)
            .with_encryption_key(&encryption);
        let package = PakPackage::open(&package_bytes, Some(&keys)).unwrap();
        let valid =
            LicenseLease::issue(&signer, "jp.example.game", "game.cold", 1_000, &policy).unwrap();
        let provider = TestLicenseProvider {
            entitlement_lease: Some(valid.clone()),
            renewal: valid.clone(),
        };
        let authorization = package.authorize(&provider, &keys, 1_001, None).unwrap();
        assert_eq!(authorization.lease, valid);
        assert!(!authorization.status.within_grace);

        let expired =
            LicenseLease::issue(&signer, "jp.example.game", "game.cold", 0, &policy).unwrap();
        let renewed =
            LicenseLease::issue(&signer, "jp.example.game", "game.cold", 100_000, &policy).unwrap();
        let provider = TestLicenseProvider {
            entitlement_lease: Some(expired),
            renewal: renewed.clone(),
        };
        let authorization = package.authorize(&provider, &keys, 100_001, None).unwrap();
        assert_eq!(authorization.lease, renewed);
    }

    #[test]
    fn pack_set_allows_optional_roles_and_rejects_missing_or_cyclic_dependencies() {
        let base = |pack_id: &str, role: PakRole, dependencies: Vec<PakDependency>| PakManifest {
            format_version: PAK_FORMAT_VERSION,
            pack_id: pack_id.to_owned(),
            game_id: "jp.example.game".to_owned(),
            role,
            subtype: "base".to_owned(),
            dependencies,
            priority: 0,
            content_root: "00".repeat(32),
            archive_hash: "11".repeat(32),
            profile: PakProfile::Dev,
            signature_key_id: None,
            encryption_key_id: None,
            license_policy: LicensePolicy::none(),
        };
        let boot = base("boot", PakRole::Boot, Vec::new());
        let overlay = base(
            "locale-ja",
            PakRole::Overlay,
            vec![PakDependency {
                pack_id: "boot".to_owned(),
                minimum_version: None,
            }],
        );
        validate_pack_set(&[boot.clone(), overlay.clone()]).unwrap();

        let missing = base(
            "missing",
            PakRole::Cold,
            vec![PakDependency {
                pack_id: "cold".to_owned(),
                minimum_version: None,
            }],
        );
        assert!(matches!(
            validate_pack_set(&[missing]),
            Err(ProtectionError::MissingDependency { .. })
        ));

        let mut cyclic_boot = boot;
        cyclic_boot.dependencies.push(PakDependency {
            pack_id: "locale-ja".to_owned(),
            minimum_version: None,
        });
        assert!(matches!(
            validate_pack_set(&[cyclic_boot, overlay]),
            Err(ProtectionError::DependencyCycle(_))
        ));
    }
}
