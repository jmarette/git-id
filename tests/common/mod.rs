//! Shared harness for the integration tests.
//!
//! Every test gets its own throwaway HOME (canonicalized, which on macOS
//! exercises the `/var` -> `/private/var` realpath logic for free) and a
//! fully scrubbed git environment, so nothing ever touches the developer's
//! real configuration.

#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Output;

use tempfile::TempDir;

/// Environment variables that could leak the developer's identity or git
/// configuration into a test.
const SCRUBBED: &[&str] = &[
    "GIT_CONFIG_GLOBAL",
    "GIT_CONFIG_SYSTEM",
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_COMMON_DIR",
    "GIT_OBJECT_DIRECTORY",
    "GIT_INDEX_FILE",
    "GIT_AUTHOR_NAME",
    "GIT_AUTHOR_EMAIL",
    "GIT_AUTHOR_DATE",
    "GIT_COMMITTER_NAME",
    "GIT_COMMITTER_EMAIL",
    "GIT_COMMITTER_DATE",
    "GIT_CONFIG_COUNT",
    "GIT_CONFIG_PARAMETERS",
    "GIT_CEILING_DIRECTORIES",
    "EMAIL",
    "VISUAL",
    "EDITOR",
    "HOME",
    "XDG_CONFIG_HOME",
    "XDG_DATA_HOME",
];

/// Strip the Windows extended-length prefix (`\\?\` or `\\?\UNC\`) emitted by
/// `fs::canonicalize`, mirroring what git-id does before matching.
#[cfg(windows)]
fn strip_verbatim(s: &str) -> String {
    if let Some(rest) = s.strip_prefix("\\\\?\\UNC\\") {
        format!("\\\\{rest}")
    } else if let Some(rest) = s.strip_prefix("\\\\?\\") {
        rest.to_string()
    } else {
        s.to_string()
    }
}

pub struct TestEnv {
    pub home: PathBuf,
    _tmp: TempDir,
}

impl TestEnv {
    pub fn new() -> Self {
        let tmp = TempDir::new().unwrap();
        // git matches gitdir patterns against realpaths; keep every expected
        // value canonical too.
        let home = fs::canonicalize(tmp.path()).unwrap();
        // On Windows `fs::canonicalize` yields an extended-length `\\?\` path;
        // strip it so HOME looks like the normal `C:\...` path real users have
        // and that git writes its config paths against.
        #[cfg(windows)]
        let home = PathBuf::from(strip_verbatim(home.to_str().unwrap()));
        Self { home, _tmp: tmp }
    }

    /// Convert a path to the same "git path" form git-id stores: forward
    /// slashes and no `\\?\` prefix on Windows; identity on Unix. Assertions
    /// compare against this rather than the native `Path::display()`.
    pub fn git_path(&self, p: &Path) -> String {
        let s = p.to_str().unwrap();
        #[cfg(windows)]
        {
            strip_verbatim(s).replace('\\', "/")
        }
        #[cfg(not(windows))]
        {
            s.to_string()
        }
    }

    pub fn config_dir(&self) -> PathBuf {
        self.home.join(".config/git-id")
    }

    pub fn routes_file(&self) -> PathBuf {
        self.config_dir().join("routes.gitconfig")
    }

    pub fn fragment(&self, name: &str) -> PathBuf {
        self.config_dir()
            .join("identities")
            .join(format!("{name}.gitconfig"))
    }

    pub fn gitconfig(&self) -> PathBuf {
        self.home.join(".gitconfig")
    }

    pub fn read(&self, path: &Path) -> String {
        fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
    }

    /// A `git-id` invocation wired to this sandbox.
    pub fn cmd(&self) -> assert_cmd::Command {
        let mut c = assert_cmd::Command::cargo_bin("git-id").unwrap();
        for key in SCRUBBED {
            c.env_remove(key);
        }
        c.env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("XDG_DATA_HOME", self.home.join(".local/share"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("LC_ALL", "C")
            .current_dir(&self.home);
        c
    }

    /// Run `git-id` with `args`, assert success, return stdout.
    pub fn ok(&self, args: &[&str]) -> String {
        let assert = self.cmd().args(args).assert().success();
        String::from_utf8_lossy(&assert.get_output().stdout).into_owned()
    }

    /// Run `git` with `args` in `dir`, within the same sandbox.
    pub fn git(&self, dir: &Path, args: &[&str]) -> Output {
        let mut c = std::process::Command::new("git");
        for key in SCRUBBED {
            c.env_remove(key);
        }
        c.env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("LC_ALL", "C")
            .current_dir(dir)
            .args(args);
        c.output().unwrap()
    }

    /// Run `git` and require success; returns trimmed stdout.
    pub fn git_ok(&self, dir: &Path, args: &[&str]) -> String {
        let out = self.git(dir, args);
        assert!(
            out.status.success(),
            "`git {}` failed in {}:\n{}",
            args.join(" "),
            dir.display(),
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// Create a `fake-bin` dir under HOME holding empty, executable files named
    /// after `bins`, and return it. Used as a PATH entry to fake which shells
    /// are "installed" without depending on the host.
    pub fn fake_bin(&self, bins: &[&str]) -> PathBuf {
        let dir = self.home.join("fake-bin");
        fs::create_dir_all(&dir).unwrap();
        for name in bins {
            let p = dir.join(name);
            fs::write(&p, "").unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
            }
            // On Windows detection probes `<name>.exe` first; create it too.
            #[cfg(windows)]
            fs::write(dir.join(format!("{name}.exe")), "").unwrap();
        }
        dir
    }

    /// `fake_bin(bins)` prepended to the inherited PATH, so detection finds the
    /// faked shells while real tools (notably `git`) stay reachable. Returns a
    /// value suitable for `.env("PATH", _)`.
    pub fn path_with_fake_bin(&self, bins: &[&str]) -> std::ffi::OsString {
        let mut paths = vec![self.fake_bin(bins)];
        paths.extend(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        ));
        std::env::join_paths(paths).unwrap()
    }

    /// Create a directory (and parents) under HOME; returns its path.
    pub fn mkdirs(&self, rel: &str) -> PathBuf {
        let dir = self.home.join(rel);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// `git init` a repository under HOME; returns its path.
    pub fn init_repo(&self, rel: &str) -> PathBuf {
        let dir = self.mkdirs(rel);
        self.git_ok(&dir, &["init", "--quiet"]);
        dir
    }

    /// Standard setup used by most scenarios: `init` (non-interactive) and
    /// one identity routed on `~/dev/work`.
    pub fn setup_work(&self) -> PathBuf {
        self.ok(&["init"]);
        self.ok(&[
            "create",
            "work",
            "--name",
            "Jane Doe",
            "--email",
            "jane@work.example",
        ]);
        let dir = self.mkdirs("dev/work");
        self.ok(&["use", "work", dir.to_str().unwrap()]);
        dir
    }

    /// Files next to `target` whose name starts with `<target>.bak-`.
    pub fn backups_of(&self, target: &Path) -> Vec<PathBuf> {
        let parent = target.parent().unwrap();
        let prefix = format!("{}.bak-", target.file_name().unwrap().to_str().unwrap());
        let mut found: Vec<PathBuf> = fs::read_dir(parent)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with(&prefix))
            })
            .collect();
        found.sort();
        found
    }
}
