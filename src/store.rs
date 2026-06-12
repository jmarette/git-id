//! Identity storage: one gitconfig fragment per identity under
//! `<config_dir>/identities/<name>.gitconfig`.
//!
//! `create` renders a fresh fragment deterministically; `edit` patches an
//! existing one through `git config --file`, so keys the user added by hand
//! (e.g. `core.sshCommand`) are preserved.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};

use crate::env::Env;
use crate::gitcfg;

pub const FRAGMENT_EXT: &str = "gitconfig";

#[derive(Debug, Clone, PartialEq)]
pub struct Identity {
    /// Slug naming the identity (and its fragment file).
    pub name: String,
    /// user.name
    pub user_name: String,
    /// user.email
    pub email: String,
    /// user.signingkey
    pub signing_key: Option<String>,
    /// commit.gpgsign
    pub sign: bool,
}

/// Partial update applied by `git-id edit` with flags.
#[derive(Debug, Default)]
pub struct IdentityPatch {
    pub user_name: Option<String>,
    pub email: Option<String>,
    /// `Some("")` removes the signing key.
    pub signing_key: Option<String>,
    pub sign: Option<bool>,
}

impl IdentityPatch {
    pub fn is_empty(&self) -> bool {
        self.user_name.is_none()
            && self.email.is_none()
            && self.signing_key.is_none()
            && self.sign.is_none()
    }
}

pub fn validate_slug(name: &str) -> Result<()> {
    ensure!(!name.is_empty(), "identity name cannot be empty");
    ensure!(
        name.len() <= 64,
        "identity name is too long (max 64 characters)"
    );
    let bytes = name.as_bytes();
    let first_ok = bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit();
    let rest_ok = bytes[1..]
        .iter()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'_'));
    ensure!(
        first_ok && rest_ok,
        "identity name must be a slug: lowercase letters, digits, `-` or `_`, \
         starting with a letter or digit (got `{name}`)"
    );
    Ok(())
}

pub fn validate_email(email: &str) -> Result<()> {
    ensure!(!email.is_empty(), "email cannot be empty");
    ensure!(
        !email.chars().any(|c| c.is_whitespace() || c.is_control()),
        "email cannot contain whitespace (got `{email}`)"
    );
    let Some((local, domain)) = email.split_once('@') else {
        bail!("email must contain a single `@` (got `{email}`)");
    };
    ensure!(
        !local.is_empty() && !domain.is_empty() && !domain.contains('@'),
        "email must look like `user@domain` (got `{email}`)"
    );
    ensure!(
        domain.contains('.') && !domain.starts_with('.') && !domain.ends_with('.'),
        "email domain must contain a dot, like `example.com` (got `{email}`)"
    );
    Ok(())
}

pub fn validate_user_name(name: &str) -> Result<()> {
    ensure!(!name.trim().is_empty(), "name cannot be empty");
    ensure!(
        !name.chars().any(char::is_control),
        "name cannot contain control characters"
    );
    Ok(())
}

pub fn fragment_path(env: &Env, name: &str) -> PathBuf {
    env.identities_dir.join(format!("{name}.{FRAGMENT_EXT}"))
}

pub fn exists(env: &Env, name: &str) -> bool {
    fragment_path(env, name).is_file()
}

/// Sorted names of all stored identities (missing directory -> empty list).
pub fn list_names(env: &Env) -> Result<Vec<String>> {
    let entries = match fs::read_dir(&env.identities_dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => {
            return Err(e).with_context(|| format!("cannot read {}", env.identities_dir.display()));
        }
    };
    let mut names = Vec::new();
    for entry in entries {
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) == Some(FRAGMENT_EXT) && path.is_file() {
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                names.push(stem.to_string());
            }
        }
    }
    names.sort();
    Ok(names)
}

pub fn load(env: &Env, name: &str) -> Result<Identity> {
    let path = fragment_path(env, name);
    ensure!(
        path.is_file(),
        "identity `{name}` does not exist (see `git-id list`, or create it with `git-id create {name}`)"
    );
    let user_name = gitcfg::get_file(&path, "user.name")?.with_context(|| {
        format!(
            "identity `{name}` has no user.name — fix it with `git-id edit {name} --name \"...\"`"
        )
    })?;
    let email = gitcfg::get_file(&path, "user.email")?.with_context(|| {
        format!(
            "identity `{name}` has no user.email — fix it with `git-id edit {name} --email ...`"
        )
    })?;
    let signing_key = gitcfg::get_file(&path, "user.signingkey")?;
    let sign = gitcfg::get_file_bool(&path, "commit.gpgsign")?.unwrap_or(false);
    Ok(Identity {
        name: name.to_string(),
        user_name,
        email,
        signing_key,
        sign,
    })
}

/// Deterministic rendering of a fresh fragment.
pub fn render_fragment(id: &Identity) -> String {
    let mut out = format!(
        "# git-id identity: {}\n[user]\n\tname = {}\n\temail = {}\n",
        id.name,
        quote_cfg_value(&id.user_name),
        quote_cfg_value(&id.email),
    );
    if let Some(key) = &id.signing_key {
        out.push_str(&format!("\tsigningkey = {}\n", quote_cfg_value(key)));
    }
    if id.sign {
        out.push_str("[commit]\n\tgpgsign = true\n");
    }
    out
}

/// Quote a gitconfig value only when needed (`#`, `;`, quotes, backslashes,
/// or leading/trailing whitespace would otherwise be mangled by git).
fn quote_cfg_value(value: &str) -> String {
    let needs_quoting = value.is_empty()
        || value.starts_with([' ', '\t'])
        || value.ends_with([' ', '\t'])
        || value.contains(['#', ';', '"', '\\']);
    if !needs_quoting {
        return value.to_string();
    }
    let escaped: String = value
        .chars()
        .flat_map(|c| match c {
            '\\' => vec!['\\', '\\'],
            '"' => vec!['\\', '"'],
            c => vec![c],
        })
        .collect();
    format!("\"{escaped}\"")
}

pub fn write_new(env: &Env, id: &Identity, force: bool) -> Result<()> {
    validate_slug(&id.name)?;
    validate_user_name(&id.user_name)?;
    validate_email(&id.email)?;
    let path = fragment_path(env, &id.name);
    if path.exists() && !force {
        bail!(
            "identity `{}` already exists ({}) — pass --force to overwrite it",
            id.name,
            path.display()
        );
    }
    atomic_write(&path, &render_fragment(id))
}

/// Apply a partial update through `git config --file`, preserving any keys
/// added to the fragment by hand.
pub fn apply_patch(env: &Env, name: &str, patch: &IdentityPatch) -> Result<()> {
    let path = fragment_path(env, name);
    ensure!(
        path.is_file(),
        "identity `{name}` does not exist (see `git-id list`)"
    );
    if let Some(user_name) = &patch.user_name {
        validate_user_name(user_name)?;
        gitcfg::set_file(&path, "user.name", user_name)?;
    }
    if let Some(email) = &patch.email {
        validate_email(email)?;
        gitcfg::set_file(&path, "user.email", email)?;
    }
    if let Some(key) = &patch.signing_key {
        if key.is_empty() {
            gitcfg::unset_file(&path, "user.signingkey")?;
        } else {
            gitcfg::set_file(&path, "user.signingkey", key)?;
        }
    }
    if let Some(sign) = patch.sign {
        gitcfg::set_file(&path, "commit.gpgsign", if sign { "true" } else { "false" })?;
    }
    Ok(())
}

pub fn remove(env: &Env, name: &str) -> Result<()> {
    let path = fragment_path(env, name);
    fs::remove_file(&path).with_context(|| format!("cannot remove {}", path.display()))
}

/// Write `contents` to `path` atomically: temp file in the same directory,
/// then rename over the target (creating parent directories as needed).
pub fn atomic_write(path: &Path, contents: &str) -> Result<()> {
    let parent = path
        .parent()
        .with_context(|| format!("`{}` has no parent directory", path.display()))?;
    fs::create_dir_all(parent).with_context(|| format!("cannot create {}", parent.display()))?;
    let mut tmp = tempfile::NamedTempFile::new_in(parent)
        .with_context(|| format!("cannot create a temporary file in {}", parent.display()))?;
    tmp.write_all(contents.as_bytes())
        .context("cannot write temporary file")?;
    tmp.persist(path)
        .map_err(|e| anyhow::Error::new(e.error))
        .with_context(|| format!("cannot atomically replace {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_validation() {
        for ok in ["work", "a", "0x", "my-client_2"] {
            assert!(validate_slug(ok).is_ok(), "{ok} should be valid");
        }
        for bad in ["", "Work", "a b", "-x", "_x", "café", "a.b", "a/b", "a@b"] {
            assert!(validate_slug(bad).is_err(), "{bad} should be invalid");
        }
        assert!(validate_slug(&"a".repeat(65)).is_err());
    }

    #[test]
    fn email_validation() {
        for ok in ["a@b.co", "jane@work.example", "x.y+z@sub.domain.org"] {
            assert!(validate_email(ok).is_ok(), "{ok} should be valid");
        }
        for bad in [
            "", "foo", "a@b", "@b.co", "a@", "a @b.co", "a@b@c.co", "a@.co", "a@co.",
        ] {
            assert!(validate_email(bad).is_err(), "{bad} should be invalid");
        }
    }

    #[test]
    fn fragment_rendering_is_deterministic() {
        let id = Identity {
            name: "work".into(),
            user_name: "Jane Doe".into(),
            email: "jane@work.example".into(),
            signing_key: None,
            sign: false,
        };
        assert_eq!(
            render_fragment(&id),
            "# git-id identity: work\n[user]\n\tname = Jane Doe\n\temail = jane@work.example\n"
        );

        let full = Identity {
            signing_key: Some("ABCDEF12".into()),
            sign: true,
            ..id
        };
        assert_eq!(
            render_fragment(&full),
            "# git-id identity: work\n[user]\n\tname = Jane Doe\n\temail = jane@work.example\n\tsigningkey = ABCDEF12\n[commit]\n\tgpgsign = true\n"
        );
    }

    #[test]
    fn values_needing_quotes_are_escaped() {
        assert_eq!(quote_cfg_value("Jane #1"), "\"Jane #1\"");
        assert_eq!(quote_cfg_value("a\"b"), "\"a\\\"b\"");
        assert_eq!(quote_cfg_value("a\\b"), "\"a\\\\b\"");
        assert_eq!(quote_cfg_value(" padded "), "\" padded \"");
        assert_eq!(quote_cfg_value("plain name"), "plain name");
    }

    #[test]
    fn rendered_fragment_roundtrips_through_git() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("x.gitconfig");
        let id = Identity {
            name: "x".into(),
            user_name: "Quote \" Back\\slash #1".into(),
            email: "q@b.co".into(),
            signing_key: None,
            sign: false,
        };
        atomic_write(&path, &render_fragment(&id)).unwrap();
        assert_eq!(
            gitcfg::get_file(&path, "user.name").unwrap().as_deref(),
            Some("Quote \" Back\\slash #1")
        );
    }

    #[test]
    fn atomic_write_replaces_and_leaves_no_temp() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("sub").join("f.txt");
        atomic_write(&path, "one").unwrap();
        atomic_write(&path, "two").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "two");
        let siblings: Vec<_> = fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(siblings, vec![std::ffi::OsString::from("f.txt")]);
    }
}
