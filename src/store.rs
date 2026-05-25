use anyhow::{Context, Result, anyhow};
use std::path::PathBuf;

use crate::object::{Event, ObjectKind, Snapshot, Tree, canonical_json, hash_bytes};
use crate::repo::Repo;

/// Read-write access to the content-addressable object store.
pub struct Store<'a> {
    pub repo: &'a Repo,
}

impl<'a> Store<'a> {
    pub fn new(repo: &'a Repo) -> Self {
        Self { repo }
    }

    fn path_for(&self, id: &str) -> PathBuf {
        self.repo.objects_dir().join(&id[..2]).join(&id[2..])
    }

    #[allow(dead_code)] // public helper, used by tests and tooling
    pub fn exists(&self, id: &str) -> bool {
        id.len() >= 4 && self.path_for(id).exists()
    }

    /// Write raw bytes; returns the BLAKE3 hex id.
    /// The first byte stored is a kind marker (1 byte) followed by content.
    /// We keep blob storage simple: just the raw content (no kind marker for blobs)
    /// but for structured objects we prefix with the kind to disambiguate during read.
    pub fn write_blob(&self, content: &[u8]) -> Result<String> {
        let id = hash_bytes(content);
        let path = self.path_for(&id);
        if path.exists() {
            return Ok(id);
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // Blob format: first byte 'B' marker, then raw bytes.
        let mut buf = Vec::with_capacity(content.len() + 1);
        buf.push(b'B');
        buf.extend_from_slice(content);
        std::fs::write(&path, &buf)?;
        Ok(id)
    }

    fn write_structured<T: serde::Serialize>(&self, kind: ObjectKind, value: &T) -> Result<String> {
        let json = canonical_json(value)?;
        // Compose stored bytes as: marker byte + canonical json
        let marker = match kind {
            ObjectKind::Tree => b'T',
            ObjectKind::Snapshot => b'S',
            ObjectKind::Event => b'E',
            ObjectKind::Blob => return Err(anyhow!("use write_blob for blobs")),
        };
        // Hash includes the marker so a tree and an identical-bytes blob never collide.
        let mut to_hash = Vec::with_capacity(json.len() + 1);
        to_hash.push(marker);
        to_hash.extend_from_slice(&json);
        let id = hash_bytes(&to_hash);
        let path = self.path_for(&id);
        if !path.exists() {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&path, &to_hash)?;
        }
        Ok(id)
    }

    pub fn write_tree(&self, tree: &Tree) -> Result<String> {
        self.write_structured(ObjectKind::Tree, tree)
    }

    pub fn write_snapshot(&self, snapshot: &Snapshot) -> Result<String> {
        self.write_structured(ObjectKind::Snapshot, snapshot)
    }

    pub fn write_event(&self, event: &Event) -> Result<String> {
        self.write_structured(ObjectKind::Event, event)
    }

    fn read_raw(&self, id: &str) -> Result<Vec<u8>> {
        let path = self.path_for(id);
        std::fs::read(&path).with_context(|| format!("reading object {}", id))
    }

    fn read_structured<T: serde::de::DeserializeOwned>(
        &self,
        id: &str,
        expected_marker: u8,
    ) -> Result<T> {
        let raw = self.read_raw(id)?;
        if raw.is_empty() {
            return Err(anyhow!("empty object {}", id));
        }
        if raw[0] != expected_marker {
            return Err(anyhow!(
                "object {} has wrong kind (expected marker {:?}, got {:?})",
                id,
                expected_marker as char,
                raw[0] as char
            ));
        }
        let value =
            serde_json::from_slice(&raw[1..]).with_context(|| format!("parsing object {}", id))?;
        Ok(value)
    }

    pub fn read_blob(&self, id: &str) -> Result<Vec<u8>> {
        let raw = self.read_raw(id)?;
        if raw.is_empty() || raw[0] != b'B' {
            return Err(anyhow!("object {} is not a blob", id));
        }
        Ok(raw[1..].to_vec())
    }

    pub fn read_tree(&self, id: &str) -> Result<Tree> {
        self.read_structured(id, b'T')
    }

    pub fn read_snapshot(&self, id: &str) -> Result<Snapshot> {
        self.read_structured(id, b'S')
    }

    pub fn read_event(&self, id: &str) -> Result<Event> {
        self.read_structured(id, b'E')
    }
}
