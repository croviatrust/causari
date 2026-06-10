use anyhow::{Context, Result, anyhow};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const CAUSARI_DIR: &str = ".causari";
pub const HEAD_FILE: &str = "HEAD";
pub const OBJECTS_DIR: &str = "objects";
pub const REFS_DIR: &str = "refs";
pub const CONFIG_FILE: &str = "config.toml";
pub const LOCK_FILE: &str = "lock";

/// A discovered Causari repository.
#[derive(Debug, Clone)]
pub struct Repo {
    /// Working tree root (the parent of `.causari/`).
    pub root: PathBuf,
    /// Path to `.causari/`.
    pub dir: PathBuf,
}

impl Repo {
    /// Discover an existing repository starting from `cwd` and walking up.
    pub fn discover() -> Result<Self> {
        let start = std::env::current_dir().context("cannot read current dir")?;
        Self::discover_from(&start)
    }

    /// Discover an existing repository starting from `start` and walking up.
    pub fn discover_from(start: &Path) -> Result<Self> {
        let mut current: Option<&Path> = Some(start);
        while let Some(p) = current {
            let candidate = p.join(CAUSARI_DIR);
            if candidate.is_dir() {
                return Ok(Self {
                    root: p.to_path_buf(),
                    dir: candidate,
                });
            }
            current = p.parent();
        }
        Err(anyhow!(
            "not a causari repository (run `re init` first to create one)"
        ))
    }

    /// Initialize a new repository at `path`.
    pub fn init(path: &Path) -> Result<Self> {
        let dir = path.join(CAUSARI_DIR);
        if dir.exists() {
            return Err(anyhow!(
                "causari repository already exists at {}",
                dir.display()
            ));
        }
        std::fs::create_dir_all(dir.join(OBJECTS_DIR))?;
        std::fs::create_dir_all(dir.join(REFS_DIR).join("sessions"))?;
        std::fs::write(dir.join(HEAD_FILE), "ref: refs/sessions/main\n")?;
        std::fs::write(
            dir.join(CONFIG_FILE),
            "# Causari configuration\nversion = 1\n",
        )?;
        // Initial empty refs file is created on first record.
        Ok(Self {
            root: path.to_path_buf(),
            dir,
        })
    }

    pub fn objects_dir(&self) -> PathBuf {
        self.dir.join(OBJECTS_DIR)
    }

    #[allow(dead_code)] // used by upcoming named-branches feature
    pub fn refs_dir(&self) -> PathBuf {
        self.dir.join(REFS_DIR)
    }

    pub fn head_path(&self) -> PathBuf {
        self.dir.join(HEAD_FILE)
    }

    /// Resolve HEAD to an event id, or None if no events recorded yet.
    pub fn head_event(&self) -> Result<Option<String>> {
        let raw = std::fs::read_to_string(self.head_path())
            .with_context(|| format!("reading {}", self.head_path().display()))?;
        let raw = raw.trim();
        if let Some(refname) = raw.strip_prefix("ref: ") {
            let p = self.dir.join(refname);
            if !p.exists() {
                return Ok(None);
            }
            let id = std::fs::read_to_string(&p)?.trim().to_string();
            if id.is_empty() {
                Ok(None)
            } else {
                Ok(Some(id))
            }
        } else if !raw.is_empty() {
            Ok(Some(raw.to_string()))
        } else {
            Ok(None)
        }
    }

    /// Update the current ref (the one HEAD points to) to a new event id.
    pub fn update_head(&self, event_id: &str) -> Result<()> {
        let raw = std::fs::read_to_string(self.head_path())?;
        let raw = raw.trim().to_string();
        if let Some(refname) = raw.strip_prefix("ref: ") {
            let p = self.dir.join(refname);
            if let Some(parent) = p.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(p, format!("{}\n", event_id))?;
        } else {
            std::fs::write(self.head_path(), format!("{}\n", event_id))?;
        }
        Ok(())
    }

    // ---------- sessions (named timelines, the branches of the DAG) ----------

    pub fn sessions_dir(&self) -> PathBuf {
        self.dir.join(REFS_DIR).join("sessions")
    }

    pub fn session_ref_path(&self, name: &str) -> PathBuf {
        self.sessions_dir().join(name)
    }

    /// Name of the session HEAD currently points to (None when detached).
    pub fn current_session(&self) -> Result<Option<String>> {
        let raw = std::fs::read_to_string(self.head_path())
            .with_context(|| format!("reading {}", self.head_path().display()))?;
        Ok(raw
            .trim()
            .strip_prefix("ref: refs/sessions/")
            .map(|s| s.to_string()))
    }

    /// Tip event of a named session, or None if the session has no events yet.
    pub fn session_head(&self, name: &str) -> Result<Option<String>> {
        let p = self.session_ref_path(name);
        if !p.exists() {
            return Ok(None);
        }
        let id = std::fs::read_to_string(&p)?.trim().to_string();
        if id.is_empty() {
            Ok(None)
        } else {
            Ok(Some(id))
        }
    }

    /// Point a named session at an event id (creates the ref if missing).
    pub fn update_session(&self, name: &str, event_id: &str) -> Result<()> {
        let p = self.session_ref_path(name);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(p, format!("{}\n", event_id))?;
        Ok(())
    }

    /// Acquire the repository write lock.
    ///
    /// Recording is a read-parent → snapshot → write-event → move-ref critical
    /// section. With multiple concurrent recorders (several `re watch`
    /// processes, agent hooks firing mid-watch, MCP calls) two writers could
    /// read the same parent and orphan one of the two events. The lock
    /// serializes the whole section. It is advisory and held via a lock file;
    /// stale locks (e.g. a killed process) expire after 30 seconds.
    pub fn lock(&self) -> Result<RepoLock> {
        let path = self.dir.join(LOCK_FILE);
        let start = Instant::now();
        loop {
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(mut f) => {
                    use std::io::Write;
                    let _ = writeln!(f, "{}", std::process::id());
                    return Ok(RepoLock { path });
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    // Break stale locks left behind by crashed processes.
                    let stale = std::fs::metadata(&path)
                        .and_then(|m| m.modified())
                        .map(|t| t.elapsed().unwrap_or_default() > Duration::from_secs(30))
                        .unwrap_or(false);
                    if stale {
                        let _ = std::fs::remove_file(&path);
                        continue;
                    }
                    if start.elapsed() > Duration::from_secs(10) {
                        return Err(anyhow!(
                            "could not acquire repository lock at {} (another recorder is running?)",
                            path.display()
                        ));
                    }
                    std::thread::sleep(Duration::from_millis(25));
                }
                Err(e) => {
                    return Err(e).with_context(|| format!("creating lock {}", path.display()));
                }
            }
        }
    }
}

/// Guard for the repository write lock; releases the lock file on drop.
pub struct RepoLock {
    path: PathBuf,
}

impl Drop for RepoLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_creates_layout_and_refuses_double_init() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = Repo::init(tmp.path()).unwrap();

        assert!(repo.objects_dir().is_dir());
        assert!(repo.sessions_dir().is_dir());
        assert!(repo.head_path().is_file());
        assert_eq!(repo.current_session().unwrap().as_deref(), Some("main"));
        assert_eq!(repo.head_event().unwrap(), None);

        assert!(Repo::init(tmp.path()).is_err());
    }

    #[test]
    fn discover_walks_up_from_nested_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        Repo::init(tmp.path()).unwrap();
        let nested = tmp.path().join("a").join("b").join("c");
        std::fs::create_dir_all(&nested).unwrap();

        let repo = Repo::discover_from(&nested).unwrap();
        // Canonicalize both sides: on Windows the temp path may come back
        // with a different case / 8.3 form.
        assert_eq!(
            repo.root.canonicalize().unwrap(),
            tmp.path().canonicalize().unwrap()
        );

        let outside = tempfile::tempdir().unwrap();
        assert!(Repo::discover_from(outside.path()).is_err());
    }

    #[test]
    fn head_follows_the_current_ref() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = Repo::init(tmp.path()).unwrap();

        repo.update_head("e1").unwrap();
        assert_eq!(repo.head_event().unwrap().as_deref(), Some("e1"));
        assert_eq!(repo.session_head("main").unwrap().as_deref(), Some("e1"));

        repo.update_head("e2").unwrap();
        assert_eq!(repo.head_event().unwrap().as_deref(), Some("e2"));
    }

    #[test]
    fn sessions_are_independent_of_head() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = Repo::init(tmp.path()).unwrap();

        repo.update_head("main-tip").unwrap();
        repo.update_session("bot1", "bot1-tip").unwrap();

        // Recording on a named session must not move HEAD, and vice versa.
        assert_eq!(repo.head_event().unwrap().as_deref(), Some("main-tip"));
        assert_eq!(
            repo.session_head("bot1").unwrap().as_deref(),
            Some("bot1-tip")
        );
        assert_eq!(repo.session_head("nope").unwrap(), None);
        assert_eq!(repo.current_session().unwrap().as_deref(), Some("main"));
    }

    #[test]
    fn detached_head_reads_back_the_raw_id() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = Repo::init(tmp.path()).unwrap();
        std::fs::write(repo.head_path(), "abcdef\n").unwrap();

        assert_eq!(repo.head_event().unwrap().as_deref(), Some("abcdef"));
        assert_eq!(repo.current_session().unwrap(), None);

        repo.update_head("123456").unwrap();
        assert_eq!(repo.head_event().unwrap().as_deref(), Some("123456"));
    }

    #[test]
    fn lock_is_exclusive_and_released_on_drop() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = Repo::init(tmp.path()).unwrap();

        let guard = repo.lock().unwrap();
        let lock_path = repo.dir.join(LOCK_FILE);
        assert!(lock_path.exists());

        // A second contender must NOT obtain the lock while it is held.
        let contender = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock_path);
        assert!(contender.is_err());

        drop(guard);
        assert!(!lock_path.exists());
        let again = repo.lock().unwrap();
        drop(again);
    }

    #[test]
    fn stale_lock_is_broken() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = Repo::init(tmp.path()).unwrap();
        let lock_path = repo.dir.join(LOCK_FILE);

        // Simulate a lock left behind by a crashed process, mtime in the past.
        std::fs::write(&lock_path, "999999\n").unwrap();
        let old = std::time::SystemTime::now() - Duration::from_secs(120);
        let f = std::fs::OpenOptions::new()
            .write(true)
            .open(&lock_path)
            .unwrap();
        f.set_modified(old).unwrap();
        drop(f);

        let guard = repo.lock().unwrap();
        drop(guard);
    }
}
