use anyhow::{Context, Result, anyhow};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const CAUSARI_DIR: &str = ".causari";
pub const HEAD_FILE: &str = "HEAD";
pub const OBJECTS_DIR: &str = "objects";
pub const REFS_DIR: &str = "refs";
pub const CONFIG_FILE: &str = "config.toml";
pub const LOCK_FILE: &str = "lock";
pub const GITIGNORE_FILE: &str = ".gitignore";
pub const GITIGNORE_ENTRY: &str = ".causari/";

/// Maximum length of a session name.
pub const MAX_SESSION_NAME_LEN: usize = 128;

/// Validate a session name. One grammar for CLI, MCP, hooks and watchers:
/// `[A-Za-z0-9._-]+`, not starting with `.` or `-`, never `HEAD`, never
/// containing path separators or control characters. Session names are used
/// as file names under `refs/sessions/`, so anything looser lets a caller
/// escape the repository (`../../x`, `/etc/passwd`, `C:\\...`).
pub fn validate_session_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(anyhow!("session name must not be empty"));
    }
    if name.len() > MAX_SESSION_NAME_LEN {
        return Err(anyhow!(
            "session name too long ({} > {} bytes)",
            name.len(),
            MAX_SESSION_NAME_LEN
        ));
    }
    if name == "HEAD" || name == "." || name == ".." {
        return Err(anyhow!("'{}' is a reserved session name", name));
    }
    if name.starts_with('.') || name.starts_with('-') {
        return Err(anyhow!(
            "session name '{}' must not start with '.' or '-'",
            name
        ));
    }
    if let Some(bad) = name
        .chars()
        .find(|c| !(c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')))
    {
        return Err(anyhow!(
            "session name '{}' contains invalid character {:?} (allowed: A-Z a-z 0-9 . _ -)",
            name,
            bad
        ));
    }
    Ok(())
}

/// Result of ensuring `.causari/` is excluded from version control.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitignoreOutcome {
    /// A `.gitignore` already excluded `.causari/`.
    AlreadyIgnored,
    /// `.causari/` was appended to an existing `.gitignore`.
    Appended,
    /// A new `.gitignore` was created with `.causari/`.
    Created,
    /// Not a git work tree and no existing `.gitignore`: nothing was written.
    NotAGitRepo,
}

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

    /// Ensure the working tree's `.gitignore` excludes `.causari/`, so that
    /// captured prompts, completions and reasoning are never committed by
    /// accident. Acts inside a git work tree, or when a `.gitignore` already
    /// exists; otherwise writes nothing and returns `NotAGitRepo`.
    pub fn ensure_gitignored(&self) -> Result<GitignoreOutcome> {
        let gitignore = self.root.join(GITIGNORE_FILE);
        let header = format!(
            "# Causari local ledger \u{2014} captured prompts, completions and reasoning.\n{}\n",
            GITIGNORE_ENTRY
        );

        if !gitignore.exists() {
            if !self.root.join(".git").exists() {
                return Ok(GitignoreOutcome::NotAGitRepo);
            }
            std::fs::write(&gitignore, header)
                .with_context(|| format!("writing {}", gitignore.display()))?;
            return Ok(GitignoreOutcome::Created);
        }

        let contents = std::fs::read_to_string(&gitignore)
            .with_context(|| format!("reading {}", gitignore.display()))?;
        let already = contents.lines().any(|l| {
            matches!(
                l.trim(),
                ".causari" | ".causari/" | "/.causari" | "/.causari/"
            )
        });
        if already {
            return Ok(GitignoreOutcome::AlreadyIgnored);
        }

        let mut updated = contents;
        if !updated.is_empty() && !updated.ends_with('\n') {
            updated.push('\n');
        }
        updated.push_str(&header);
        std::fs::write(&gitignore, updated)
            .with_context(|| format!("writing {}", gitignore.display()))?;
        Ok(GitignoreOutcome::Appended)
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

    /// Path of a session ref. Fails on names that do not satisfy
    /// [`validate_session_name`], so no caller can build a path outside
    /// `refs/sessions/`. Also refuses to follow a symlinked ref.
    pub fn session_ref_path(&self, name: &str) -> Result<PathBuf> {
        validate_session_name(name)?;
        let p = self.sessions_dir().join(name);
        if let Ok(meta) = std::fs::symlink_metadata(&p) {
            if meta.file_type().is_symlink() {
                return Err(anyhow!(
                    "session ref {} is a symlink; refusing to follow it",
                    p.display()
                ));
            }
        }
        Ok(p)
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
        let p = self.session_ref_path(name)?;
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
        let p = self.session_ref_path(name)?;
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(p, format!("{}\n", event_id))?;
        Ok(())
    }

    /// Compare-and-swap on a session tip: advance `name` to `event_id` only
    /// if its current tip is still `expected`. This is the last line of
    /// defence against a lost update when two recorders read the same parent
    /// (e.g. after a lock was wrongly broken): the second writer fails
    /// loudly instead of silently orphaning the first one's event.
    pub fn update_session_if(
        &self,
        name: &str,
        expected: Option<&str>,
        event_id: &str,
    ) -> Result<()> {
        let current = self.session_head(name)?;
        if current.as_deref() != expected {
            return Err(anyhow!(
                "session '{}' moved concurrently (expected tip {}, found {}); event {} written but not linked — re-run the record",
                name,
                expected.map(|s| &s[..s.len().min(10)]).unwrap_or("<none>"),
                current
                    .as_deref()
                    .map(|s| &s[..s.len().min(10)])
                    .unwrap_or("<none>"),
                &event_id[..event_id.len().min(10)]
            ));
        }
        self.update_session(name, event_id)
    }

    /// Compare-and-swap on the current HEAD ref. See [`Self::update_session_if`].
    pub fn update_head_if(&self, expected: Option<&str>, event_id: &str) -> Result<()> {
        let current = self.head_event()?;
        if current.as_deref() != expected {
            return Err(anyhow!(
                "HEAD moved concurrently (expected tip {}, found {}); event {} written but not linked — re-run the record",
                expected.map(|s| &s[..s.len().min(10)]).unwrap_or("<none>"),
                current
                    .as_deref()
                    .map(|s| &s[..s.len().min(10)])
                    .unwrap_or("<none>"),
                &event_id[..event_id.len().min(10)]
            ));
        }
        self.update_head(event_id)
    }

    /// Acquire the repository write lock.
    ///
    /// Recording is a read-parent → snapshot → write-event → move-ref critical
    /// section. With multiple concurrent recorders (several `re watch`
    /// processes, agent hooks firing mid-watch, MCP calls) two writers could
    /// read the same parent and orphan one of the two events. The lock
    /// serializes the section. It is advisory and held via a lock file that records the
    /// owner's pid. A lock is only broken when its owner is provably dead
    /// (where the platform lets us check) or when it is older than
    /// [`LOCK_HARD_EXPIRY`]; a mere 30-second age is NOT proof the owner
    /// died. Ref updates additionally use compare-and-swap, so even a wrongly
    /// broken lock cannot silently orphan an event.
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
                    if lock_is_stale(&path) {
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

/// Locks older than this are broken regardless of owner liveness (a hung
/// recorder must not wedge the repository forever).
pub const LOCK_HARD_EXPIRY: Duration = Duration::from_secs(10 * 60);

/// Below this age a lock is never questioned.
pub const LOCK_SOFT_THRESHOLD: Duration = Duration::from_secs(30);

/// Is the lock at `path` safe to break?
///
/// * owner pid readable and provably dead → stale
/// * owner pid readable and provably alive → NOT stale (any age below hard expiry)
/// * liveness unknown (other OS, unreadable file) → stale only after hard expiry
fn lock_is_stale(path: &Path) -> bool {
    let age = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .map(|t| t.elapsed().unwrap_or_default())
        .unwrap_or_default();
    if age > LOCK_HARD_EXPIRY {
        return true;
    }
    if age < LOCK_SOFT_THRESHOLD {
        // Fresh lock: the owner is almost certainly mid-record. Do not even
        // probe; probing costs a process spawn on some platforms.
        return false;
    }
    let owner = std::fs::read_to_string(path)
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok());
    match owner.map(process_alive) {
        Some(Some(false)) => true,
        Some(Some(true)) => false,
        // Unknown liveness: be conservative, only the hard expiry breaks it.
        _ => false,
    }
}

/// Best-effort liveness probe. `None` when the platform gives no cheap answer.
fn process_alive(pid: u32) -> Option<bool> {
    if pid == std::process::id() {
        return Some(true);
    }
    #[cfg(target_os = "linux")]
    {
        return Some(Path::new("/proc").join(pid.to_string()).exists());
    }
    #[cfg(all(unix, not(target_os = "linux")))]
    {
        // `kill -0` sends no signal; it only checks the pid exists (and that
        // we may signal it — for our own uid's processes that is always true).
        let status = std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stderr(std::process::Stdio::null())
            .status()
            .ok()?;
        return Some(status.success());
    }
    #[cfg(windows)]
    {
        // tasklist is always present; a missing pid yields an INFO line, not a match.
        let out = std::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {}", pid), "/NH", "/FO", "CSV"])
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        return Some(text.contains(&format!("\"{}\"", pid)));
    }
    #[allow(unreachable_code)]
    None
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
    fn session_names_cannot_escape_refs_dir() {
        // Regression (F01): an absolute or traversing session name used to
        // be joined verbatim and create a file outside the repository.
        let tmp = tempfile::tempdir().unwrap();
        let repo = Repo::init(tmp.path()).unwrap();
        let outside = tmp.path().join("outside.txt");
        let abs = outside.to_string_lossy().to_string();

        for bad in [
            abs.as_str(),
            "../../escape",
            "a/b",
            "a\\b",
            "",
            ".hidden",
            "-flag",
            "HEAD",
            "..",
            "with space",
            "nul\0byte",
            "\u{e9}t\u{e9}",
        ] {
            assert!(validate_session_name(bad).is_err(), "accepted {:?}", bad);
            assert!(
                repo.session_ref_path(bad).is_err(),
                "path built for {:?}",
                bad
            );
            assert!(repo.update_session(bad, "deadbeef").is_err());
            assert!(repo.session_head(bad).is_err());
        }
        assert!(!outside.exists(), "file created outside the repo");

        for good in ["main", "bot-2", "claude.code_v1", "A", &"x".repeat(128)] {
            assert!(validate_session_name(good).is_ok(), "rejected {:?}", good);
        }
        assert!(validate_session_name(&"x".repeat(129)).is_err());
    }

    #[test]
    fn fresh_lock_held_by_live_process_is_not_broken() {
        // Regression (F04): a lock a few seconds old used to be breakable
        // after 30s of age even with its owner alive. A fresh lock is never
        // questioned; our own pid is always considered alive.
        let tmp = tempfile::tempdir().unwrap();
        let repo = Repo::init(tmp.path()).unwrap();
        let path = repo.dir.join(LOCK_FILE);
        std::fs::write(&path, format!("{}\n", std::process::id())).unwrap();
        assert!(!lock_is_stale(&path));
        assert_eq!(process_alive(std::process::id()), Some(true));
    }

    #[test]
    fn compare_and_swap_refuses_a_moved_tip() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = Repo::init(tmp.path()).unwrap();
        repo.update_session("bot", "aaaa").unwrap();
        assert!(repo.update_session_if("bot", Some("zzzz"), "bbbb").is_err());
        assert_eq!(repo.session_head("bot").unwrap().as_deref(), Some("aaaa"));
        repo.update_session_if("bot", Some("aaaa"), "bbbb").unwrap();
        assert_eq!(repo.session_head("bot").unwrap().as_deref(), Some("bbbb"));

        assert!(repo.update_head_if(Some("nope"), "cccc").is_err());
        repo.update_head_if(None, "cccc").unwrap();
        assert_eq!(repo.head_event().unwrap().as_deref(), Some("cccc"));
    }

    #[test]
    fn ensure_gitignored_creates_appends_and_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        // Simulate a git work tree so the helper is willing to create .gitignore.
        std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
        let repo = Repo::init(tmp.path()).unwrap();
        let gi = tmp.path().join(GITIGNORE_FILE);

        // First call creates .gitignore carrying the entry.
        assert_eq!(repo.ensure_gitignored().unwrap(), GitignoreOutcome::Created);
        assert!(std::fs::read_to_string(&gi).unwrap().contains(".causari/"));

        // Second call is idempotent.
        assert_eq!(
            repo.ensure_gitignored().unwrap(),
            GitignoreOutcome::AlreadyIgnored
        );

        // Appends to a pre-existing, unrelated .gitignore without clobbering it.
        std::fs::write(&gi, "target/\n").unwrap();
        assert_eq!(
            repo.ensure_gitignored().unwrap(),
            GitignoreOutcome::Appended
        );
        let body = std::fs::read_to_string(&gi).unwrap();
        assert!(body.contains("target/"));
        assert!(body.lines().any(|l| l.trim() == ".causari/"));
    }

    #[test]
    fn ensure_gitignored_is_noop_outside_a_git_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = Repo::init(tmp.path()).unwrap();
        assert_eq!(
            repo.ensure_gitignored().unwrap(),
            GitignoreOutcome::NotAGitRepo
        );
        assert!(!tmp.path().join(GITIGNORE_FILE).exists());
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
