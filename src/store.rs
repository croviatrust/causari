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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::TreeEntry;
    use std::collections::BTreeMap;

    fn test_repo() -> (tempfile::TempDir, Repo) {
        let tmp = tempfile::tempdir().unwrap();
        let repo = Repo::init(tmp.path()).unwrap();
        (tmp, repo)
    }

    fn sample_event(parent: Option<String>) -> Event {
        Event {
            schema: "causari.event.v0.2".into(),
            parent,
            agent: Some("test-agent".into()),
            model: None,
            tool: Some("edit".into()),
            message: Some("hello".into()),
            prompt: Some("do the thing".into()),
            reasoning: None,
            reads: vec![],
            writes: vec!["a.txt".into()],
            tokens_in: None,
            tokens_out: None,
            cost_usd: None,
            pre_snapshot: "pre".into(),
            post_snapshot: "post".into(),
            exit_code: None,
            created_at: "2026-01-01T00:00:00Z".into(),
        }
    }

    #[test]
    fn blob_roundtrip_and_dedup() {
        let (_tmp, repo) = test_repo();
        let store = Store::new(&repo);

        let id1 = store.write_blob(b"hello world").unwrap();
        let id2 = store.write_blob(b"hello world").unwrap();
        assert_eq!(id1, id2, "identical content must dedup to one object");
        assert_eq!(store.read_blob(&id1).unwrap(), b"hello world");
        assert!(store.exists(&id1));
    }

    #[test]
    fn structured_roundtrips() {
        let (_tmp, repo) = test_repo();
        let store = Store::new(&repo);

        let mut entries = BTreeMap::new();
        entries.insert(
            "main.rs".to_string(),
            TreeEntry {
                kind: "blob".into(),
                id: "ff".repeat(32),
            },
        );
        let tree_id = store.write_tree(&Tree { entries }).unwrap();
        let tree = store.read_tree(&tree_id).unwrap();
        assert_eq!(tree.entries["main.rs"].kind, "blob");

        let snap_id = store
            .write_snapshot(&Snapshot {
                tree: tree_id.clone(),
                created_at: "2026-01-01T00:00:00Z".into(),
            })
            .unwrap();
        assert_eq!(store.read_snapshot(&snap_id).unwrap().tree, tree_id);

        let ev_id = store.write_event(&sample_event(None)).unwrap();
        let ev = store.read_event(&ev_id).unwrap();
        assert_eq!(ev.message.as_deref(), Some("hello"));
        assert_eq!(ev.parent, None);
    }

    #[test]
    fn kind_markers_prevent_cross_kind_reads() {
        let (_tmp, repo) = test_repo();
        let store = Store::new(&repo);

        let blob_id = store.write_blob(b"{}").unwrap();
        assert!(store.read_tree(&blob_id).is_err());
        assert!(store.read_snapshot(&blob_id).is_err());
        assert!(store.read_event(&blob_id).is_err());

        let ev_id = store.write_event(&sample_event(None)).unwrap();
        assert!(store.read_blob(&ev_id).is_err());
        assert!(store.read_tree(&ev_id).is_err());
    }

    #[test]
    fn blob_and_structured_with_same_bytes_do_not_collide() {
        let (_tmp, repo) = test_repo();
        let store = Store::new(&repo);

        // A blob whose content happens to be a tree's canonical JSON must
        // still get a different id (the kind marker is hashed).
        let tree = Tree {
            entries: BTreeMap::new(),
        };
        let tree_id = store.write_tree(&tree).unwrap();
        let json = crate::object::canonical_json(&tree).unwrap();
        let blob_id = store.write_blob(&json).unwrap();
        assert_ne!(tree_id, blob_id);
    }

    #[test]
    fn reading_missing_object_fails_cleanly() {
        let (_tmp, repo) = test_repo();
        let store = Store::new(&repo);
        assert!(store.read_event(&"a".repeat(64)).is_err());
        assert!(!store.exists(&"a".repeat(64)));
    }
}
