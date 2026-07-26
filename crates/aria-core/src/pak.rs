//! Portable, deterministic asset archives for Aria V3.
//!
//! `ARIAPAK4` deliberately has no platform profile, encryption, or build key.
//! A game data bundle must be byte-identical on Windows, Linux, and Web; a
//! generic Player cannot keep a decryption key secret anyway.  The format
//! therefore provides corruption detection, not DRM or publisher
//! authentication.
//!
//! All integer fields are little endian. The on-disk layout is:
//!
//! ```text
//! [fixed header]
//! [UTF-8 game ID]
//! [binary index: sorted entries, then chunks]
//! [chunk payload]
//! [BLAKE3 of every preceding byte]
//! ```
//!
//! The fixed header contains the index and payload lengths, counts, and a
//! content root. Entries are sorted by their full 32-byte canonical-path hash.
//! Each asset points to one contiguous range of chunks. Chunks are likewise
//! contiguous in the payload, which lets the reader reject holes, overlaps,
//! and aliasing before an asset is decoded.

use std::collections::BTreeMap;
use std::io::{Read, Write};

use flate2::Compression;
use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;
use serde::Deserialize;
use thiserror::Error;

use crate::compiler::{normalize_logical_path, portable_path_key};

/// Magic bytes for the portable ARIAPAK4 format.
pub const PAK_MAGIC: [u8; 8] = *b"ARIAPAK4";
/// Current ARIAPAK format major version.
pub const PAK_FORMAT: u16 = 4;
/// A single uncompressed chunk never exceeds 256 KiB.
pub const PAK_CHUNK_MAX_RAW_SIZE: usize = 256 * 1024;

const MAX_CHUNK_STORED_SIZE: usize = PAK_CHUNK_MAX_RAW_SIZE + 1024;
const HEADER_SIZE: usize = 72;
const ENTRY_SIZE: usize = 80;
const CHUNK_SIZE: usize = 84;
const CHECKSUM_SIZE: usize = 32;
const MAX_GAME_ID_SIZE: usize = 4 * 1024;
const MAX_INDEX_SIZE: usize = 64 * 1024 * 1024;
const MAX_ENTRY_COUNT: usize = 100_000;
const MAX_CHUNK_COUNT: usize = 500_000;
const MAX_ASSET_SIZE: u64 = 1024 * 1024 * 1024;
const MAX_TOTAL_RAW_SIZE: u64 = 8 * 1024 * 1024 * 1024;
const MAX_PAYLOAD_SIZE: u64 = 4 * 1024 * 1024 * 1024;
const INDEX_FLAGS: u16 = 0;
const CODEC_STORED: u8 = 0;
const CODEC_DEFLATE: u8 = 1;

/// An asset supplied to [`PakArchive::build`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetInput {
    /// Project-relative, UTF-8 logical path.
    pub logical_path: String,
    /// Uncompressed asset data.
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum CompressionKind {
    Stored = CODEC_STORED,
    Deflate = CODEC_DEFLATE,
}

impl CompressionKind {
    fn from_byte(value: u8) -> Result<Self, PakError> {
        match value {
            CODEC_STORED => Ok(Self::Stored),
            CODEC_DEFLATE => Ok(Self::Deflate),
            _ => Err(PakError::InvalidIndex(format!(
                "unknown chunk codec {value}"
            ))),
        }
    }
}

#[derive(Debug, Clone)]
struct PakEntry {
    path_hash: [u8; 32],
    content_checksum: [u8; 32],
    raw_size: u64,
    first_chunk: u32,
    chunk_count: u32,
}

#[derive(Debug, Clone)]
struct PakChunk {
    offset: u64,
    stored_size: u32,
    raw_size: u32,
    compression: CompressionKind,
    stored_checksum: [u8; 32],
    raw_checksum: [u8; 32],
}

/// An integrity-checked portable asset archive.
#[derive(Debug, Clone)]
pub struct PakArchive {
    game_id: String,
    content_root: [u8; 32],
    payload: Vec<u8>,
    entries: BTreeMap<[u8; 32], PakEntry>,
    chunks: Vec<PakChunk>,
}

impl PakArchive {
    /// Builds a deterministic ARIAPAK4 archive.
    ///
    /// Input paths are normalized, duplicate normalized paths are rejected,
    /// then entries are sorted by their full canonical-path hashes. The same
    /// game ID and logical asset bytes always produce the same archive bytes,
    /// regardless of input ordering or host platform.
    pub fn build(game_id: impl Into<String>, assets: Vec<AssetInput>) -> Result<Vec<u8>, PakError> {
        let game_id = game_id.into();
        validate_game_id(&game_id)?;

        let mut normalized = BTreeMap::new();
        let mut portable_names = BTreeMap::new();
        for asset in assets {
            let path =
                normalize_logical_path(&asset.logical_path).map_err(PakError::InvalidPath)?;
            let portable_name = portable_path_key(&path).map_err(PakError::InvalidPath)?;
            if let Some(existing) = portable_names.insert(portable_name, path.clone())
                && existing != path
            {
                return Err(PakError::PortablePathCollision { existing, path });
            }
            if u64::try_from(asset.bytes.len()).map_err(|_| PakError::ArchiveTooLarge)?
                > MAX_ASSET_SIZE
            {
                return Err(PakError::AssetTooLarge(path));
            }
            if normalized.insert(path.clone(), asset.bytes).is_some() {
                return Err(PakError::DuplicatePath(path));
            }
        }
        if normalized.len() > MAX_ENTRY_COUNT {
            return Err(PakError::ArchiveTooLarge);
        }

        // A BTreeMap over the full 256-bit digest establishes the canonical
        // index order. The collision check is primarily a defensive invariant:
        // BLAKE3 collisions are not expected, but silently aliasing assets is
        // never acceptable.
        let mut hashed = BTreeMap::<[u8; 32], (String, Vec<u8>)>::new();
        for (path, bytes) in normalized {
            let hash = canonical_path_hash(&path);
            if let Some((existing, _)) = hashed.get(&hash) {
                return Err(PakError::PathHashCollision(format!(
                    "'{existing}' and '{path}'"
                )));
            }
            hashed.insert(hash, (path, bytes));
        }

        let mut entries = Vec::new();
        let mut chunks = Vec::new();
        let mut payload = Vec::new();
        let mut total_raw_size = 0_u64;

        for (path_hash, (_path, bytes)) in hashed {
            let raw_size = u64::try_from(bytes.len()).map_err(|_| PakError::ArchiveTooLarge)?;
            total_raw_size = total_raw_size
                .checked_add(raw_size)
                .ok_or(PakError::ArchiveTooLarge)?;
            if total_raw_size > MAX_TOTAL_RAW_SIZE {
                return Err(PakError::ArchiveTooLarge);
            }

            let first_chunk = u32::try_from(chunks.len()).map_err(|_| PakError::ArchiveTooLarge)?;
            for raw in bytes.chunks(PAK_CHUNK_MAX_RAW_SIZE) {
                if chunks.len() >= MAX_CHUNK_COUNT {
                    return Err(PakError::ArchiveTooLarge);
                }
                let deflated = deflate(raw)?;
                let (stored, compression) = if deflated.len() < raw.len() {
                    (deflated, CompressionKind::Deflate)
                } else {
                    (raw.to_vec(), CompressionKind::Stored)
                };
                let offset = u64::try_from(payload.len()).map_err(|_| PakError::ArchiveTooLarge)?;
                let stored_size =
                    u32::try_from(stored.len()).map_err(|_| PakError::ArchiveTooLarge)?;
                let raw_size = u32::try_from(raw.len()).map_err(|_| PakError::ArchiveTooLarge)?;
                append_payload(&mut payload, &stored)?;
                chunks.push(PakChunk {
                    offset,
                    stored_size,
                    raw_size,
                    compression,
                    stored_checksum: *blake3::hash(&stored).as_bytes(),
                    raw_checksum: *blake3::hash(raw).as_bytes(),
                });
            }
            let chunk_count = u32::try_from(chunks.len())
                .map_err(|_| PakError::ArchiveTooLarge)?
                .checked_sub(first_chunk)
                .ok_or(PakError::ArchiveTooLarge)?;
            entries.push(PakEntry {
                path_hash,
                content_checksum: *blake3::hash(&bytes).as_bytes(),
                raw_size,
                first_chunk,
                chunk_count,
            });
        }

        let index_size = index_size(entries.len(), chunks.len())?;
        let content_root = calculate_content_root(&game_id, &entries);
        let game_id_len = u32::try_from(game_id.len()).map_err(|_| PakError::ArchiveTooLarge)?;
        let entry_count = u32::try_from(entries.len()).map_err(|_| PakError::ArchiveTooLarge)?;
        let chunk_count = u32::try_from(chunks.len()).map_err(|_| PakError::ArchiveTooLarge)?;
        let payload_len = u64::try_from(payload.len()).map_err(|_| PakError::ArchiveTooLarge)?;
        let archive_without_checksum = HEADER_SIZE
            .checked_add(game_id.len())
            .and_then(|size| size.checked_add(index_size))
            .and_then(|size| size.checked_add(payload.len()))
            .ok_or(PakError::ArchiveTooLarge)?;
        if u64::try_from(archive_without_checksum).map_err(|_| PakError::ArchiveTooLarge)?
            > MAX_PAYLOAD_SIZE
                .checked_add(
                    u64::try_from(HEADER_SIZE + MAX_GAME_ID_SIZE + MAX_INDEX_SIZE)
                        .expect("format limits fit u64"),
                )
                .ok_or(PakError::ArchiveTooLarge)?
        {
            return Err(PakError::ArchiveTooLarge);
        }

        let total_size = archive_without_checksum
            .checked_add(CHECKSUM_SIZE)
            .ok_or(PakError::ArchiveTooLarge)?;
        let mut archive = Vec::new();
        archive
            .try_reserve_exact(total_size)
            .map_err(|_| PakError::ArchiveTooLarge)?;
        archive.extend_from_slice(&PAK_MAGIC);
        push_u16(&mut archive, PAK_FORMAT);
        push_u16(&mut archive, INDEX_FLAGS);
        push_u32(&mut archive, game_id_len);
        push_u32(&mut archive, entry_count);
        push_u32(&mut archive, chunk_count);
        push_u64(
            &mut archive,
            u64::try_from(index_size).expect("index size fits u64"),
        );
        push_u64(&mut archive, payload_len);
        archive.extend_from_slice(&content_root);
        debug_assert_eq!(archive.len(), HEADER_SIZE);

        archive.extend_from_slice(game_id.as_bytes());
        for entry in &entries {
            encode_entry(&mut archive, entry);
        }
        for chunk in &chunks {
            encode_chunk(&mut archive, chunk);
        }
        debug_assert_eq!(archive.len(), HEADER_SIZE + game_id.len() + index_size);
        archive.extend_from_slice(&payload);
        debug_assert_eq!(archive.len(), archive_without_checksum);
        let archive_checksum = blake3::hash(&archive);
        archive.extend_from_slice(archive_checksum.as_bytes());
        Ok(archive)
    }

    /// Opens and structurally validates an ARIAPAK4 archive.
    ///
    /// Opening verifies the archive checksum, fixed header, canonical index
    /// ordering, exact payload coverage, and every stored chunk checksum.
    /// Decompression and raw checksums are verified lazily by [`Self::read`].
    pub fn open(bytes: &[u8]) -> Result<Self, PakError> {
        // The adapter-owned PAK4 envelope has a distinct magic and can carry
        // a plaintext `dev` pack. Core may unwrap that profile for data-only
        // providers, but it deliberately refuses signed/protected
        // material; those profiles are authenticated by Native/Web adapters.
        if bytes.starts_with(b"ARIAPK4P") {
            return open_dev_package(bytes);
        }
        if bytes.len() < HEADER_SIZE + CHECKSUM_SIZE {
            return Err(PakError::Truncated);
        }
        let checked_len = bytes.len() - CHECKSUM_SIZE;
        let checked = &bytes[..checked_len];
        if blake3::hash(checked).as_bytes() != &bytes[checked_len..] {
            return Err(PakError::ChecksumMismatch);
        }

        let mut cursor = 0;
        if take::<8>(checked, &mut cursor)? != PAK_MAGIC {
            return Err(PakError::InvalidMagic);
        }
        let version = take_u16(checked, &mut cursor)?;
        if version != PAK_FORMAT {
            return Err(PakError::UnsupportedVersion(version));
        }
        let flags = take_u16(checked, &mut cursor)?;
        if flags != INDEX_FLAGS {
            return Err(PakError::UnsupportedFlags(flags));
        }
        let game_id_len =
            usize::try_from(take_u32(checked, &mut cursor)?).map_err(|_| PakError::Truncated)?;
        let entry_count =
            usize::try_from(take_u32(checked, &mut cursor)?).map_err(|_| PakError::Truncated)?;
        let chunk_count =
            usize::try_from(take_u32(checked, &mut cursor)?).map_err(|_| PakError::Truncated)?;
        let index_len = usize::try_from(take_u64(checked, &mut cursor)?)
            .map_err(|_| PakError::IndexTooLarge)?;
        let payload_len = usize::try_from(take_u64(checked, &mut cursor)?)
            .map_err(|_| PakError::ArchiveTooLarge)?;
        let content_root = take::<32>(checked, &mut cursor)?;
        debug_assert_eq!(cursor, HEADER_SIZE);

        if game_id_len == 0 || game_id_len > MAX_GAME_ID_SIZE {
            return Err(PakError::InvalidIndex("invalid game ID length".to_owned()));
        }
        if entry_count > MAX_ENTRY_COUNT || chunk_count > MAX_CHUNK_COUNT {
            return Err(PakError::ArchiveTooLarge);
        }
        if u64::try_from(payload_len).map_err(|_| PakError::ArchiveTooLarge)? > MAX_PAYLOAD_SIZE {
            return Err(PakError::ArchiveTooLarge);
        }
        let expected_index_len = index_size(entry_count, chunk_count)?;
        if index_len != expected_index_len {
            return Err(PakError::InvalidIndex(format!(
                "index length is {index_len}, expected {expected_index_len}"
            )));
        }

        let game_id_bytes = take_slice(checked, &mut cursor, game_id_len)?;
        let game_id = std::str::from_utf8(game_id_bytes)
            .map_err(PakError::InvalidGameIdUtf8)?
            .to_owned();
        validate_game_id(&game_id)?;
        let index = take_slice(checked, &mut cursor, index_len)?;
        let payload = take_slice(checked, &mut cursor, payload_len)?;
        if cursor != checked.len() {
            return Err(PakError::Truncated);
        }

        let (entries, chunks) = decode_index(index, entry_count, chunk_count)?;
        validate_entry_chunks(&entries, &chunks)?;
        validate_payload_coverage(payload, &chunks)?;
        if calculate_content_root(&game_id, &entries) != content_root {
            return Err(PakError::ContentRootMismatch);
        }

        let mut entry_map = BTreeMap::new();
        for entry in entries {
            if entry_map.insert(entry.path_hash, entry).is_some() {
                return Err(PakError::InvalidIndex(
                    "duplicate canonical path hash in index".to_owned(),
                ));
            }
        }
        Ok(Self {
            game_id,
            content_root,
            payload: payload.to_vec(),
            entries: entry_map,
            chunks,
        })
    }

    /// Game ID bound into the archive content root.
    #[must_use]
    pub fn game_id(&self) -> &str {
        &self.game_id
    }

    /// Number of logical assets in the archive.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the archive has no logical assets.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Hex-encoded deterministic root of the game ID and raw logical assets.
    #[must_use]
    pub fn content_root_hex(&self) -> String {
        hex::encode(self.content_root)
    }

    /// Reads, decompresses, and verifies one logical asset.
    pub fn read(&self, logical_path: &str) -> Result<Vec<u8>, PakError> {
        let path = normalize_logical_path(logical_path).map_err(PakError::InvalidPath)?;
        if path != logical_path {
            return Err(PakError::InvalidPath(format!(
                "logical asset path '{logical_path}' is not canonical NFC '/' spelling; use '{path}'"
            )));
        }
        let path_hash = canonical_path_hash(&path);
        let entry = self
            .entries
            .get(&path_hash)
            .ok_or_else(|| PakError::MissingAsset(path.clone()))?;
        let result_capacity =
            usize::try_from(entry.raw_size).map_err(|_| PakError::AssetTooLarge(path.clone()))?;
        let mut result = Vec::new();
        result
            .try_reserve_exact(result_capacity)
            .map_err(|_| PakError::AssetTooLarge(path.clone()))?;

        let first_chunk = usize::try_from(entry.first_chunk).map_err(|_| PakError::Truncated)?;
        let chunk_count = usize::try_from(entry.chunk_count).map_err(|_| PakError::Truncated)?;
        let end_chunk = first_chunk
            .checked_add(chunk_count)
            .filter(|end| *end <= self.chunks.len())
            .ok_or(PakError::Truncated)?;
        for (relative_index, chunk) in self.chunks[first_chunk..end_chunk].iter().enumerate() {
            let chunk_index = first_chunk + relative_index;
            let (start, end) = payload_range(chunk, self.payload.len())?;
            let stored = &self.payload[start..end];
            if blake3::hash(stored).as_bytes() != &chunk.stored_checksum {
                return Err(PakError::ChunkStoredChecksumMismatch { chunk: chunk_index });
            }
            let expected_raw_size =
                usize::try_from(chunk.raw_size).map_err(|_| PakError::Truncated)?;
            let raw = match chunk.compression {
                CompressionKind::Stored => {
                    if stored.len() != expected_raw_size {
                        return Err(PakError::InvalidCompressedSize {
                            expected: expected_raw_size,
                            actual: stored.len(),
                        });
                    }
                    stored.to_vec()
                }
                CompressionKind::Deflate => inflate(stored, expected_raw_size)?,
            };
            if blake3::hash(&raw).as_bytes() != &chunk.raw_checksum {
                return Err(PakError::ChunkRawChecksumMismatch {
                    path,
                    chunk: chunk_index,
                });
            }
            result.extend_from_slice(&raw);
        }

        if result.len() != result_capacity
            || blake3::hash(&result).as_bytes() != &entry.content_checksum
        {
            return Err(PakError::AssetChecksumMismatch(path));
        }
        Ok(result)
    }
}

#[derive(Debug, Deserialize)]
struct DevPackageManifest {
    format_version: u16,
    game_id: String,
    profile: String,
    content_root: String,
    archive_hash: String,
}

/// Reads only the unencrypted adapter envelope. This keeps existing Core
/// asset providers useful for `dev` builds without putting any key or crypto
/// contract into the deterministic VM crate.
fn open_dev_package(bytes: &[u8]) -> Result<PakArchive, PakError> {
    const PACKAGE_HEADER_SIZE: usize = 72;
    const PACKAGE_DESCRIPTOR_SIZE: usize = 112;
    if bytes.len() < PACKAGE_HEADER_SIZE {
        return Err(PakError::Truncated);
    }
    let mut cursor = 8;
    let version = take_u16(bytes, &mut cursor)?;
    if version != PAK_FORMAT {
        return Err(PakError::UnsupportedVersion(version));
    }
    let profile = take::<1>(bytes, &mut cursor)?[0];
    if profile != 0 {
        return Err(PakError::UnsupportedFlags(u16::from(profile)));
    }
    if take::<1>(bytes, &mut cursor)?[0] != 0 {
        return Err(PakError::UnsupportedFlags(0x8000));
    }
    let manifest_len =
        usize::try_from(take_u32(bytes, &mut cursor)?).map_err(|_| PakError::ArchiveTooLarge)?;
    let chunk_count =
        usize::try_from(take_u32(bytes, &mut cursor)?).map_err(|_| PakError::ArchiveTooLarge)?;
    let descriptor_len =
        usize::try_from(take_u32(bytes, &mut cursor)?).map_err(|_| PakError::IndexTooLarge)?;
    let signature_len = usize::from(take_u16(bytes, &mut cursor)?);
    if take_u16(bytes, &mut cursor)? != 0 {
        return Err(PakError::InvalidIndex(
            "package header reserved bytes are non-zero".to_owned(),
        ));
    }
    let payload_len =
        usize::try_from(take_u64(bytes, &mut cursor)?).map_err(|_| PakError::ArchiveTooLarge)?;
    let content_root = take::<32>(bytes, &mut cursor)?;
    if take::<4>(bytes, &mut cursor)? != [0; 4]
        || descriptor_len != chunk_count.saturating_mul(PACKAGE_DESCRIPTOR_SIZE)
        || signature_len != 0
    {
        return Err(PakError::InvalidIndex(
            "invalid plaintext PAK4 package header".to_owned(),
        ));
    }
    let manifest_bytes = take_slice(bytes, &mut cursor, manifest_len)?;
    let manifest: DevPackageManifest = serde_json::from_slice(manifest_bytes).map_err(|error| {
        PakError::InvalidIndex(format!("invalid PAK4 package manifest: {error}"))
    })?;
    if manifest.format_version != PAK_FORMAT || manifest.profile != "dev" {
        return Err(PakError::UnsupportedFlags(0x8000));
    }
    if hex::encode(content_root) != manifest.content_root {
        return Err(PakError::ContentRootMismatch);
    }
    let descriptors = take_slice(bytes, &mut cursor, descriptor_len)?;
    let payload = take_slice(bytes, &mut cursor, payload_len)?;
    if cursor != bytes.len() {
        return Err(PakError::Truncated);
    }
    let mut descriptor_cursor = 0;
    let mut expected_offset = 0_u64;
    let mut archive_bytes = Vec::new();
    for chunk_index in 0..chunk_count {
        let raw_size = take_u32(descriptors, &mut descriptor_cursor)?;
        let encoded_size = take_u32(descriptors, &mut descriptor_cursor)?;
        let stored_size = take_u32(descriptors, &mut descriptor_cursor)?;
        let offset = take_u64(descriptors, &mut descriptor_cursor)?;
        let compression = take::<1>(descriptors, &mut descriptor_cursor)?[0];
        let encrypted = take::<1>(descriptors, &mut descriptor_cursor)?[0];
        if take::<2>(descriptors, &mut descriptor_cursor)? != [0; 2]
            || raw_size == 0
            || usize::try_from(raw_size).unwrap_or(usize::MAX) > PAK_CHUNK_MAX_RAW_SIZE
            || encoded_size == 0
            || stored_size != encoded_size
            || encrypted != 0
            || offset != expected_offset
        {
            return Err(PakError::InvalidIndex(format!(
                "invalid plaintext PAK4 package chunk {chunk_index}"
            )));
        }
        let _nonce = take::<24>(descriptors, &mut descriptor_cursor)?;
        let stored_checksum = take::<32>(descriptors, &mut descriptor_cursor)?;
        let raw_checksum = take::<32>(descriptors, &mut descriptor_cursor)?;
        let start = usize::try_from(offset).map_err(|_| PakError::ArchiveTooLarge)?;
        let end = start
            .checked_add(usize::try_from(stored_size).map_err(|_| PakError::ArchiveTooLarge)?)
            .ok_or(PakError::ArchiveTooLarge)?;
        let stored = payload.get(start..end).ok_or(PakError::Truncated)?;
        if blake3::hash(stored).as_bytes() != &stored_checksum {
            return Err(PakError::ChunkStoredChecksumMismatch { chunk: chunk_index });
        }
        let raw = match compression {
            CODEC_STORED => stored.to_vec(),
            CODEC_DEFLATE => inflate(
                stored,
                usize::try_from(raw_size).map_err(|_| PakError::ArchiveTooLarge)?,
            )?,
            _ => {
                return Err(PakError::InvalidIndex(
                    "unknown plaintext PAK4 package codec".to_owned(),
                ));
            }
        };
        if raw.len() != usize::try_from(raw_size).map_err(|_| PakError::ArchiveTooLarge)?
            || blake3::hash(&raw).as_bytes() != &raw_checksum
        {
            return Err(PakError::ChunkRawChecksumMismatch {
                path: manifest.game_id.clone(),
                chunk: chunk_index,
            });
        }
        archive_bytes.extend_from_slice(&raw);
        expected_offset = offset
            .checked_add(u64::from(stored_size))
            .ok_or(PakError::ArchiveTooLarge)?;
    }
    if usize::try_from(expected_offset).map_err(|_| PakError::ArchiveTooLarge)? != payload.len()
        || blake3::hash(&archive_bytes).to_hex().as_str() != manifest.archive_hash
    {
        return Err(PakError::ChecksumMismatch);
    }
    let archive = PakArchive::open(&archive_bytes)?;
    if archive.game_id() != manifest.game_id || archive.content_root_hex() != manifest.content_root
    {
        return Err(PakError::ContentRootMismatch);
    }
    Ok(archive)
}

fn validate_game_id(game_id: &str) -> Result<(), PakError> {
    if game_id.trim().is_empty() {
        return Err(PakError::InvalidGameId("game ID is empty".to_owned()));
    }
    if game_id.len() > MAX_GAME_ID_SIZE {
        return Err(PakError::InvalidGameId("game ID is too long".to_owned()));
    }
    Ok(())
}

fn canonical_path_hash(path: &str) -> [u8; 32] {
    *blake3::hash(path.as_bytes()).as_bytes()
}

fn calculate_content_root(game_id: &str, entries: &[PakEntry]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new_derive_key("AriaEngine ARIAPAK4 content root");
    hasher.update(
        &u32::try_from(game_id.len())
            .expect("validated game ID length fits u32")
            .to_le_bytes(),
    );
    hasher.update(game_id.as_bytes());
    hasher.update(
        &u32::try_from(entries.len())
            .expect("format entry count fits u32")
            .to_le_bytes(),
    );
    for entry in entries {
        hasher.update(&entry.path_hash);
        hasher.update(&entry.content_checksum);
        hasher.update(&entry.raw_size.to_le_bytes());
    }
    *hasher.finalize().as_bytes()
}

fn index_size(entry_count: usize, chunk_count: usize) -> Result<usize, PakError> {
    if entry_count > MAX_ENTRY_COUNT || chunk_count > MAX_CHUNK_COUNT {
        return Err(PakError::ArchiveTooLarge);
    }
    let size = entry_count
        .checked_mul(ENTRY_SIZE)
        .and_then(|size| {
            chunk_count
                .checked_mul(CHUNK_SIZE)
                .and_then(|chunks| size.checked_add(chunks))
        })
        .ok_or(PakError::IndexTooLarge)?;
    if size > MAX_INDEX_SIZE {
        return Err(PakError::IndexTooLarge);
    }
    Ok(size)
}

fn append_payload(payload: &mut Vec<u8>, stored: &[u8]) -> Result<(), PakError> {
    let next_len = payload
        .len()
        .checked_add(stored.len())
        .ok_or(PakError::ArchiveTooLarge)?;
    if u64::try_from(next_len).map_err(|_| PakError::ArchiveTooLarge)? > MAX_PAYLOAD_SIZE {
        return Err(PakError::ArchiveTooLarge);
    }
    payload
        .try_reserve(stored.len())
        .map_err(|_| PakError::ArchiveTooLarge)?;
    payload.extend_from_slice(stored);
    Ok(())
}

fn encode_entry(output: &mut Vec<u8>, entry: &PakEntry) {
    output.extend_from_slice(&entry.path_hash);
    output.extend_from_slice(&entry.content_checksum);
    push_u64(output, entry.raw_size);
    push_u32(output, entry.first_chunk);
    push_u32(output, entry.chunk_count);
}

fn encode_chunk(output: &mut Vec<u8>, chunk: &PakChunk) {
    output.extend_from_slice(&chunk.offset.to_le_bytes());
    output.extend_from_slice(&chunk.stored_size.to_le_bytes());
    output.extend_from_slice(&chunk.raw_size.to_le_bytes());
    output.push(chunk.compression as u8);
    output.extend_from_slice(&[0, 0, 0]);
    output.extend_from_slice(&chunk.stored_checksum);
    output.extend_from_slice(&chunk.raw_checksum);
}

fn decode_index(
    index: &[u8],
    entry_count: usize,
    chunk_count: usize,
) -> Result<(Vec<PakEntry>, Vec<PakChunk>), PakError> {
    let mut cursor = 0;
    let mut entries = Vec::new();
    entries
        .try_reserve_exact(entry_count)
        .map_err(|_| PakError::ArchiveTooLarge)?;
    let mut previous_hash = None;
    for _ in 0..entry_count {
        let path_hash = take::<32>(index, &mut cursor)?;
        if previous_hash.is_some_and(|previous| previous >= path_hash) {
            return Err(PakError::InvalidIndex(
                "entry path hashes are not strictly sorted".to_owned(),
            ));
        }
        previous_hash = Some(path_hash);
        let content_checksum = take::<32>(index, &mut cursor)?;
        let raw_size = take_u64(index, &mut cursor)?;
        let first_chunk = take_u32(index, &mut cursor)?;
        let chunks_for_entry = take_u32(index, &mut cursor)?;
        if raw_size > MAX_ASSET_SIZE {
            return Err(PakError::InvalidIndex(
                "asset exceeds maximum uncompressed size".to_owned(),
            ));
        }
        let chunk_count_for_entry =
            usize::try_from(chunks_for_entry).map_err(|_| PakError::Truncated)?;
        if chunk_count_for_entry > max_chunks_per_asset() {
            return Err(PakError::InvalidIndex(
                "asset declares too many chunks".to_owned(),
            ));
        }
        if (raw_size == 0) != (chunks_for_entry == 0) {
            return Err(PakError::InvalidIndex(
                "empty assets must have zero chunks and non-empty assets must have chunks"
                    .to_owned(),
            ));
        }
        entries.push(PakEntry {
            path_hash,
            content_checksum,
            raw_size,
            first_chunk,
            chunk_count: chunks_for_entry,
        });
    }

    let mut chunks = Vec::new();
    chunks
        .try_reserve_exact(chunk_count)
        .map_err(|_| PakError::ArchiveTooLarge)?;
    for _ in 0..chunk_count {
        let offset = take_u64(index, &mut cursor)?;
        let stored_size = take_u32(index, &mut cursor)?;
        let raw_size = take_u32(index, &mut cursor)?;
        let compression = CompressionKind::from_byte(take::<1>(index, &mut cursor)?[0])?;
        if take::<3>(index, &mut cursor)? != [0, 0, 0] {
            return Err(PakError::InvalidIndex(
                "chunk reserved bytes must be zero".to_owned(),
            ));
        }
        let stored_checksum = take::<32>(index, &mut cursor)?;
        let raw_checksum = take::<32>(index, &mut cursor)?;
        if raw_size == 0
            || usize::try_from(raw_size).map_err(|_| PakError::Truncated)? > PAK_CHUNK_MAX_RAW_SIZE
        {
            return Err(PakError::InvalidIndex(
                "chunk raw size is outside the 1..=256 KiB limit".to_owned(),
            ));
        }
        if stored_size == 0 {
            return Err(PakError::InvalidIndex(
                "non-empty chunks must have stored bytes".to_owned(),
            ));
        }
        if usize::try_from(stored_size).map_err(|_| PakError::Truncated)? > MAX_CHUNK_STORED_SIZE {
            return Err(PakError::InvalidIndex(
                "chunk stored size exceeds the format limit".to_owned(),
            ));
        }
        if compression == CompressionKind::Stored && stored_size != raw_size {
            return Err(PakError::InvalidIndex(
                "stored chunks must use equal stored and raw sizes".to_owned(),
            ));
        }
        chunks.push(PakChunk {
            offset,
            stored_size,
            raw_size,
            compression,
            stored_checksum,
            raw_checksum,
        });
    }
    if cursor != index.len() {
        return Err(PakError::InvalidIndex(
            "index has trailing or unparsed bytes".to_owned(),
        ));
    }
    Ok((entries, chunks))
}

fn max_chunks_per_asset() -> usize {
    usize::try_from(
        MAX_ASSET_SIZE
            .div_ceil(u64::try_from(PAK_CHUNK_MAX_RAW_SIZE).expect("chunk size fits u64")),
    )
    .expect("asset chunk bound fits usize")
}

fn validate_entry_chunks(entries: &[PakEntry], chunks: &[PakChunk]) -> Result<(), PakError> {
    let mut expected_first = 0_usize;
    let mut total_raw_size = 0_u64;
    for entry in entries {
        let first_chunk = usize::try_from(entry.first_chunk).map_err(|_| PakError::Truncated)?;
        let chunk_count = usize::try_from(entry.chunk_count).map_err(|_| PakError::Truncated)?;
        if first_chunk != expected_first {
            return Err(PakError::InvalidIndex(
                "entry chunk ranges are not contiguous".to_owned(),
            ));
        }
        let end_chunk = first_chunk
            .checked_add(chunk_count)
            .filter(|end| *end <= chunks.len())
            .ok_or_else(|| {
                PakError::InvalidIndex("entry chunk range is out of bounds".to_owned())
            })?;
        let mut entry_raw_size = 0_u64;
        for chunk in &chunks[first_chunk..end_chunk] {
            entry_raw_size = entry_raw_size
                .checked_add(u64::from(chunk.raw_size))
                .ok_or(PakError::ArchiveTooLarge)?;
        }
        if entry_raw_size != entry.raw_size {
            return Err(PakError::InvalidIndex(
                "entry raw size differs from its chunk sizes".to_owned(),
            ));
        }
        total_raw_size = total_raw_size
            .checked_add(entry_raw_size)
            .ok_or(PakError::ArchiveTooLarge)?;
        if total_raw_size > MAX_TOTAL_RAW_SIZE {
            return Err(PakError::ArchiveTooLarge);
        }
        expected_first = end_chunk;
    }
    if expected_first != chunks.len() {
        return Err(PakError::InvalidIndex(
            "one or more chunks are not owned by an entry".to_owned(),
        ));
    }
    Ok(())
}

fn validate_payload_coverage(payload: &[u8], chunks: &[PakChunk]) -> Result<(), PakError> {
    let mut expected_offset = 0_usize;
    for (index, chunk) in chunks.iter().enumerate() {
        let (start, end) = payload_range(chunk, payload.len())?;
        if start != expected_offset {
            return Err(PakError::InvalidIndex(format!(
                "payload chunk {index} does not begin at the expected offset"
            )));
        }
        if blake3::hash(&payload[start..end]).as_bytes() != &chunk.stored_checksum {
            return Err(PakError::ChunkStoredChecksumMismatch { chunk: index });
        }
        expected_offset = end;
    }
    if expected_offset != payload.len() {
        return Err(PakError::InvalidIndex(
            "payload contains bytes not covered by chunks".to_owned(),
        ));
    }
    Ok(())
}

fn payload_range(chunk: &PakChunk, payload_len: usize) -> Result<(usize, usize), PakError> {
    let start = usize::try_from(chunk.offset).map_err(|_| PakError::Truncated)?;
    let size = usize::try_from(chunk.stored_size).map_err(|_| PakError::Truncated)?;
    let end = start.checked_add(size).ok_or(PakError::Truncated)?;
    if end > payload_len {
        return Err(PakError::InvalidIndex(
            "chunk payload range is outside the payload".to_owned(),
        ));
    }
    Ok((start, end))
}

fn deflate(bytes: &[u8]) -> Result<Vec<u8>, PakError> {
    // The level is fixed as part of ARIAPAK4's reproducibility contract.
    let mut encoder = DeflateEncoder::new(Vec::new(), Compression::new(9));
    encoder.write_all(bytes).map_err(PakError::Compression)?;
    encoder.finish().map_err(PakError::Compression)
}

fn inflate(bytes: &[u8], expected: usize) -> Result<Vec<u8>, PakError> {
    let decoder = DeflateDecoder::new(bytes);
    let mut output = Vec::new();
    output
        .try_reserve_exact(expected)
        .map_err(|_| PakError::ArchiveTooLarge)?;
    decoder
        .take(
            u64::try_from(expected)
                .expect("chunk size fits u64")
                .saturating_add(1),
        )
        .read_to_end(&mut output)
        .map_err(PakError::Compression)?;
    if output.len() != expected {
        return Err(PakError::InvalidCompressedSize {
            expected,
            actual: output.len(),
        });
    }
    Ok(output)
}

fn push_u16(output: &mut Vec<u8>, value: u16) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn push_u32(output: &mut Vec<u8>, value: u32) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn push_u64(output: &mut Vec<u8>, value: u64) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn take_u16(bytes: &[u8], cursor: &mut usize) -> Result<u16, PakError> {
    Ok(u16::from_le_bytes(take(bytes, cursor)?))
}

fn take_u32(bytes: &[u8], cursor: &mut usize) -> Result<u32, PakError> {
    Ok(u32::from_le_bytes(take(bytes, cursor)?))
}

fn take_u64(bytes: &[u8], cursor: &mut usize) -> Result<u64, PakError> {
    Ok(u64::from_le_bytes(take(bytes, cursor)?))
}

fn take_slice<'a>(
    bytes: &'a [u8],
    cursor: &mut usize,
    length: usize,
) -> Result<&'a [u8], PakError> {
    let end = cursor.checked_add(length).ok_or(PakError::Truncated)?;
    let slice = bytes.get(*cursor..end).ok_or(PakError::Truncated)?;
    *cursor = end;
    Ok(slice)
}

fn take<const N: usize>(bytes: &[u8], cursor: &mut usize) -> Result<[u8; N], PakError> {
    take_slice(bytes, cursor, N)?
        .try_into()
        .map_err(|_| PakError::Truncated)
}

/// Errors returned while building, opening, or reading an ARIAPAK4 archive.
#[derive(Debug, Error)]
pub enum PakError {
    #[error("invalid ARIAPAK4 magic")]
    InvalidMagic,
    #[error("truncated or length-mismatched pak")]
    Truncated,
    #[error("pak archive checksum mismatch")]
    ChecksumMismatch,
    #[error("unsupported pak format version {0}")]
    UnsupportedVersion(u16),
    #[error("unsupported pak header flags {0:#x}")]
    UnsupportedFlags(u16),
    #[error("invalid pak game ID: {0}")]
    InvalidGameId(String),
    #[error("invalid UTF-8 game ID: {0}")]
    InvalidGameIdUtf8(std::str::Utf8Error),
    #[error("invalid pak index: {0}")]
    InvalidIndex(String),
    #[error("pak index is too large")]
    IndexTooLarge,
    #[error("pak archive is too large")]
    ArchiveTooLarge,
    #[error("invalid logical path: {0}")]
    InvalidPath(String),
    #[error("duplicate logical asset path '{0}'")]
    DuplicatePath(String),
    #[error("asset paths '{existing}' and '{path}' collide on a case-insensitive filesystem")]
    PortablePathCollision { existing: String, path: String },
    #[error("canonical path-hash collision between {0}")]
    PathHashCollision(String),
    #[error("asset is too large: '{0}'")]
    AssetTooLarge(String),
    #[error("missing asset '{0}'")]
    MissingAsset(String),
    #[error("pak content root does not match the index")]
    ContentRootMismatch,
    #[error("stored checksum mismatch for pak chunk {chunk}")]
    ChunkStoredChecksumMismatch { chunk: usize },
    #[error("raw checksum mismatch for pak asset '{path}', chunk {chunk}")]
    ChunkRawChecksumMismatch { path: String, chunk: usize },
    #[error("asset checksum mismatch for '{0}'")]
    AssetChecksumMismatch(String),
    #[error("invalid decompressed size: expected {expected}, got {actual}")]
    InvalidCompressedSize { expected: usize, actual: usize },
    #[error("pak compression error: {0}")]
    Compression(std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(path: &str, bytes: Vec<u8>) -> AssetInput {
        AssetInput {
            logical_path: path.to_owned(),
            bytes,
        }
    }

    fn pseudorandom_bytes(length: usize) -> Vec<u8> {
        let mut state = 0x1234_5678_9abc_def0_u64;
        (0..length)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                state as u8
            })
            .collect()
    }

    fn read_u32(bytes: &[u8], offset: usize) -> u32 {
        u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
    }

    fn read_u64(bytes: &[u8], offset: usize) -> u64 {
        u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap())
    }

    fn write_u32(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn write_u64(bytes: &mut [u8], offset: usize, value: u64) {
        bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }

    fn reseal(bytes: &mut [u8]) {
        let checksum_offset = bytes.len() - CHECKSUM_SIZE;
        let checksum = blake3::hash(&bytes[..checksum_offset]);
        bytes[checksum_offset..].copy_from_slice(checksum.as_bytes());
    }

    fn index_start(bytes: &[u8]) -> usize {
        HEADER_SIZE + usize::try_from(read_u32(bytes, 12)).unwrap()
    }

    #[test]
    fn archive_is_reproducible_across_input_ordering() {
        let inputs = vec![
            asset("assets/z.txt", b"zebra".to_vec()),
            asset("assets/a.txt", vec![b'a'; 8_192]),
            asset("assets/empty.bin", Vec::new()),
        ];
        let mut reverse = inputs.clone();
        reverse.reverse();

        let first = PakArchive::build("jp.example.game", inputs).unwrap();
        let second = PakArchive::build("jp.example.game", reverse).unwrap();
        assert_eq!(first, second);

        let archive = PakArchive::open(&first).unwrap();
        assert_eq!(archive.len(), 3);
        assert!(!archive.content_root_hex().is_empty());
    }

    #[test]
    fn reads_stored_and_deflated_multi_chunk_assets() {
        let repeated = vec![b'w'; PAK_CHUNK_MAX_RAW_SIZE * 2 + 19];
        let noisy = pseudorandom_bytes(PAK_CHUNK_MAX_RAW_SIZE);
        let encoded = PakArchive::build(
            "jp.example.game",
            vec![
                asset("assets/repeated.bin", repeated.clone()),
                asset("assets/noisy.bin", noisy.clone()),
            ],
        )
        .unwrap();
        let archive = PakArchive::open(&encoded).unwrap();
        assert_eq!(archive.read("assets/repeated.bin").unwrap(), repeated);
        assert_eq!(archive.read("assets/noisy.bin").unwrap(), noisy);
        assert!(
            archive
                .chunks
                .iter()
                .any(|chunk| chunk.compression == CompressionKind::Deflate)
        );
        assert!(
            archive
                .chunks
                .iter()
                .any(|chunk| chunk.compression == CompressionKind::Stored)
        );
    }

    #[test]
    fn detects_archive_and_stored_chunk_corruption() {
        let encoded = PakArchive::build(
            "jp.example.game",
            vec![asset("assets/data.bin", vec![b'x'; 32_768])],
        )
        .unwrap();
        let payload_start = index_start(&encoded)
            + usize::try_from(read_u64(&encoded, 24)).expect("index length fits usize");

        let mut corrupted = encoded.clone();
        corrupted[payload_start] ^= 0x20;
        assert!(matches!(
            PakArchive::open(&corrupted),
            Err(PakError::ChecksumMismatch)
        ));

        reseal(&mut corrupted);
        assert!(matches!(
            PakArchive::open(&corrupted),
            Err(PakError::ChunkStoredChecksumMismatch { .. })
        ));
    }

    #[test]
    fn rejects_overlapping_chunks_and_bad_chunk_sizes_even_when_resealed() {
        let encoded = PakArchive::build(
            "jp.example.game",
            vec![asset(
                "assets/data.bin",
                vec![b'x'; PAK_CHUNK_MAX_RAW_SIZE * 2],
            )],
        )
        .unwrap();
        assert_eq!(read_u32(&encoded, 16), 1);
        assert_eq!(read_u32(&encoded, 20), 2);
        let first_chunk = index_start(&encoded) + ENTRY_SIZE;
        let second_chunk = first_chunk + CHUNK_SIZE;

        let mut overlap = encoded.clone();
        write_u64(&mut overlap, second_chunk, 0);
        reseal(&mut overlap);
        assert!(matches!(
            PakArchive::open(&overlap),
            Err(PakError::InvalidIndex(_))
        ));

        let mut oversized = encoded;
        write_u32(
            &mut oversized,
            first_chunk + 12,
            u32::try_from(PAK_CHUNK_MAX_RAW_SIZE + 1).unwrap(),
        );
        reseal(&mut oversized);
        assert!(matches!(
            PakArchive::open(&oversized),
            Err(PakError::InvalidIndex(_))
        ));
    }

    #[test]
    fn rejects_duplicate_normalized_paths() {
        let error = PakArchive::build(
            "jp.example.game",
            vec![
                asset("assets/a/../same.bin", vec![1]),
                asset("assets/same.bin", vec![2]),
            ],
        )
        .unwrap_err();
        assert!(matches!(error, PakError::DuplicatePath(_)));
    }

    #[test]
    fn rejects_case_insensitive_asset_path_collisions() {
        let error = PakArchive::build(
            "jp.example.game",
            vec![
                asset("assets/Mio.png", vec![1]),
                asset("assets/mio.png", vec![2]),
            ],
        )
        .unwrap_err();
        assert!(matches!(error, PakError::PortablePathCollision { .. }));
    }

    #[test]
    fn reads_only_exact_canonical_logical_paths() {
        let encoded = PakArchive::build(
            "jp.example.game",
            vec![asset("assets/font.ttf", vec![1, 2, 3])],
        )
        .unwrap();
        let archive = PakArchive::open(&encoded).unwrap();
        assert_eq!(archive.read("assets/font.ttf").unwrap(), vec![1, 2, 3]);
        assert!(matches!(
            archive.read("assets\\font.ttf"),
            Err(PakError::InvalidPath(_))
        ));
    }
}
