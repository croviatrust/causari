use anyhow::{Context, Result, anyhow};
use std::path::{Path, PathBuf};

pub const CAUSARI_DIR: &str = ".causari";
pub const HEAD_FILE: &str = "HEAD";
pub const OBJECTS_DIR: &str = "objects";
pub const REFS_DIR: &str = "refs";
pub const CONFIG_FILE: &str = "config.toml";

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
        let mut current: Option<&Path> = Some(&start);
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
}
