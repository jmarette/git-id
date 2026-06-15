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
    /// gpg.format (signing backend): `openpgp`, `ssh` or `x509`.
    pub format: Option<String>,
    /// core.sshCommand: the command git runs in place of `ssh` for this
    /// identity's git operations (per-identity SSH key).
    pub ssh_command: Option<String>,
}

/// Partial update applied by `git-id edit` with flags.
#[derive(Debug, Default)]
pub struct IdentityPatch {
    pub user_name: Option<String>,
    pub email: Option<String>,
    /// `Some("")` removes the signing key.
    pub signing_key: Option<String>,
    pub sign: Option<bool>,
    /// `Some("")` removes gpg.format.
    pub format: Option<String>,
    /// `Some("")` removes core.sshCommand.
    pub ssh_command: Option<String>,
}

impl IdentityPatch {
    pub fn is_empty(&self) -> bool {
        self.user_name.is_none()
            && self.email.is_none()
            && self.signing_key.is_none()
            && self.sign.is_none()
            && self.format.is_none()
            && self.ssh_command.is_none()
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

/// A signing key is free-form (GPG id, SSH key, `key::` spec…), but it must not
/// carry control characters: a newline in particular would let the value break
/// out of its line in the rendered fragment and inject an arbitrary gitconfig
/// section (e.g. `core.sshCommand`). Callers pass only non-empty keys here;
/// an empty value means "no key" / "remove the key".
pub fn validate_signing_key(key: &str) -> Result<()> {
    ensure!(
        !key.chars().any(char::is_control),
        "signing key cannot contain control characters"
    );
    Ok(())
}

/// `core.sshCommand` is free-form (a shell command line), but like a signing
/// key it must not carry control characters: a newline would let the value
/// break out of its line and inject an arbitrary gitconfig section.
/// `quote_cfg_value` escapes such characters at render time too; this is the
/// first line of defense. Callers pass only non-empty commands here.
pub fn validate_ssh_command(cmd: &str) -> Result<()> {
    ensure!(
        !cmd.chars().any(char::is_control),
        "ssh command cannot contain control characters"
    );
    Ok(())
}

/// `gpg.format` must be one of git's three signing backends.
pub fn validate_format(format: &str) -> Result<()> {
    ensure!(
        matches!(format, "openpgp" | "ssh" | "x509"),
        "signing format must be one of openpgp, ssh or x509 (got `{format}`)"
    );
    Ok(())
}

/// Build the `core.sshCommand` value for `--ssh-key <path>`: force git to use
/// exactly this key (`IdentitiesOnly=yes`, so an agent key can't shadow it).
/// The path is single-quoted for the shell git runs the command through (a
/// POSIX `sh` everywhere, including Git for Windows), so spaces and shell
/// metacharacters in the path stay literal.
pub fn ssh_command_for_key(path: &str) -> String {
    format!("ssh -i {} -o IdentitiesOnly=yes", shell_single_quote(path))
}

/// POSIX single-quote: wrap in `'…'`, encoding any embedded `'` as `'\''`.
fn shell_single_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Resolve the `core.sshCommand` value from the two mutually exclusive
/// `--ssh-key` / `--ssh-command` flags (clap already forbids passing both).
/// `--ssh-key` is sugar for a deterministic command; `--ssh-command` is stored
/// verbatim. Returns `None` when neither is given. Empty values are rejected —
/// removal is `git-id edit --no-ssh`, handled by the caller.
pub fn resolve_ssh_command(
    ssh_key: Option<&str>,
    ssh_command: Option<&str>,
) -> Result<Option<String>> {
    if let Some(path) = ssh_key {
        ensure!(!path.is_empty(), "ssh key path cannot be empty");
        validate_ssh_command(path)?;
        Ok(Some(ssh_command_for_key(path)))
    } else if let Some(cmd) = ssh_command {
        ensure!(!cmd.is_empty(), "ssh command cannot be empty");
        validate_ssh_command(cmd)?;
        Ok(Some(cmd.to_string()))
    } else {
        Ok(None)
    }
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
    let format = gitcfg::get_file(&path, "gpg.format")?;
    let ssh_command = gitcfg::get_file(&path, "core.sshCommand")?;
    Ok(Identity {
        name: name.to_string(),
        user_name,
        email,
        signing_key,
        sign,
        format,
        ssh_command,
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
    if let Some(format) = &id.format {
        out.push_str(&format!("[gpg]\n\tformat = {}\n", quote_cfg_value(format)));
    }
    if id.sign {
        out.push_str("[commit]\n\tgpgsign = true\n");
    }
    if let Some(cmd) = &id.ssh_command {
        out.push_str(&format!(
            "[core]\n\tsshCommand = {}\n",
            quote_cfg_value(cmd)
        ));
    }
    out
}

/// Quote a gitconfig value only when needed (`#`, `;`, quotes, backslashes,
/// control characters, or leading/trailing whitespace would otherwise be
/// mangled by — or break out of — the line git reads back). Control characters
/// are escaped to git's own forms (`\n`, `\t`, `\b`) so the value round-trips
/// and can never inject a new section; this is defense in depth on top of the
/// `validate_*` checks, so no field rendered through here can ever inject.
fn quote_cfg_value(value: &str) -> String {
    let needs_quoting = value.is_empty()
        || value.starts_with([' ', '\t'])
        || value.ends_with([' ', '\t'])
        || value.contains(['#', ';', '"', '\\'])
        || value.chars().any(char::is_control);
    if !needs_quoting {
        return value.to_string();
    }
    let escaped: String = value
        .chars()
        .flat_map(|c| match c {
            '\\' => vec!['\\', '\\'],
            '"' => vec!['\\', '"'],
            '\n' => vec!['\\', 'n'],
            '\t' => vec!['\\', 't'],
            '\u{8}' => vec!['\\', 'b'],
            c => vec![c],
        })
        .collect();
    format!("\"{escaped}\"")
}

pub fn write_new(env: &Env, id: &Identity, force: bool) -> Result<()> {
    validate_slug(&id.name)?;
    validate_user_name(&id.user_name)?;
    validate_email(&id.email)?;
    if let Some(key) = &id.signing_key {
        validate_signing_key(key)?;
    }
    if let Some(format) = &id.format {
        validate_format(format)?;
    }
    if let Some(cmd) = &id.ssh_command {
        validate_ssh_command(cmd)?;
    }
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
    // Validate every present field before writing any, so a rejected patch is a
    // true no-op instead of leaving an earlier field already committed.
    if let Some(user_name) = &patch.user_name {
        validate_user_name(user_name)?;
    }
    if let Some(email) = &patch.email {
        validate_email(email)?;
    }
    if let Some(key) = &patch.signing_key {
        if !key.is_empty() {
            validate_signing_key(key)?;
        }
    }
    if let Some(format) = &patch.format {
        if !format.is_empty() {
            validate_format(format)?;
        }
    }
    if let Some(cmd) = &patch.ssh_command {
        if !cmd.is_empty() {
            validate_ssh_command(cmd)?;
        }
    }
    if let Some(user_name) = &patch.user_name {
        gitcfg::set_file(&path, "user.name", user_name)?;
    }
    if let Some(email) = &patch.email {
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
    if let Some(format) = &patch.format {
        if format.is_empty() {
            gitcfg::unset_file(&path, "gpg.format")?;
        } else {
            gitcfg::set_file(&path, "gpg.format", format)?;
        }
    }
    if let Some(cmd) = &patch.ssh_command {
        if cmd.is_empty() {
            gitcfg::unset_file(&path, "core.sshCommand")?;
        } else {
            gitcfg::set_file(&path, "core.sshCommand", cmd)?;
        }
    }
    Ok(())
}

pub fn remove(env: &Env, name: &str) -> Result<()> {
    let path = fragment_path(env, name);
    fs::remove_file(&path).with_context(|| format!("cannot remove {}", path.display()))
}

/// Atomically write `contents` to `path` only if it differs from what is
/// already there. Returns `true` when it wrote (the file was missing or held
/// different content), `false` when the file already matched and was left
/// untouched. A missing or unreadable file counts as "differs" and triggers a
/// write, so callers get an idempotent install that still refreshes stale files.
pub fn write_if_changed(path: &Path, contents: &str) -> Result<bool> {
    if let Ok(existing) = fs::read_to_string(path) {
        if existing == contents {
            return Ok(false);
        }
    }
    atomic_write(path, contents)?;
    Ok(true)
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
            format: None,
            ssh_command: None,
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
    fn renders_gpg_format_and_ssh_command_sections() {
        let id = Identity {
            name: "work".into(),
            user_name: "Jane Doe".into(),
            email: "jane@work.example".into(),
            signing_key: Some("ABCDEF12".into()),
            sign: true,
            format: Some("ssh".into()),
            ssh_command: Some("ssh -i '/home/jane/.ssh/id_work' -o IdentitiesOnly=yes".into()),
        };
        assert_eq!(
            render_fragment(&id),
            "# git-id identity: work\n[user]\n\tname = Jane Doe\n\temail = jane@work.example\n\
             \tsigningkey = ABCDEF12\n[gpg]\n\tformat = ssh\n[commit]\n\tgpgsign = true\n\
             [core]\n\tsshCommand = ssh -i '/home/jane/.ssh/id_work' -o IdentitiesOnly=yes\n"
        );
    }

    #[test]
    fn format_validation() {
        for ok in ["openpgp", "ssh", "x509"] {
            assert!(validate_format(ok).is_ok(), "{ok} should be valid");
        }
        for bad in ["", "OpenPGP", "gpg", "rsa", "ssh "] {
            assert!(validate_format(bad).is_err(), "{bad} should be invalid");
        }
    }

    #[test]
    fn ssh_command_for_key_shell_quotes_the_path() {
        assert_eq!(
            ssh_command_for_key("/home/jane/.ssh/id_work"),
            "ssh -i '/home/jane/.ssh/id_work' -o IdentitiesOnly=yes"
        );
        // Spaces stay literal inside the single quotes.
        assert_eq!(
            ssh_command_for_key("/home/jane/My Keys/id"),
            "ssh -i '/home/jane/My Keys/id' -o IdentitiesOnly=yes"
        );
        // An embedded apostrophe is closed, escaped, and reopened.
        assert_eq!(
            ssh_command_for_key("/home/o'brien/id"),
            "ssh -i '/home/o'\\''brien/id' -o IdentitiesOnly=yes"
        );
    }

    #[test]
    fn ssh_command_rejects_control_chars() {
        assert!(validate_ssh_command("ssh -i ~/.ssh/id -p 2222").is_ok());
        assert!(validate_ssh_command("ssh\n[user]\n\temail = evil@x.co").is_err());
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
    fn signing_key_rejects_control_chars() {
        for ok in ["ABCDEF12", "key::ssh-ed25519 AAAAC3Nza", "0x1234"] {
            assert!(validate_signing_key(ok).is_ok(), "{ok} should be valid");
        }
        assert!(validate_signing_key("KEY\n[core]\n\tsshCommand = x").is_err());
        assert!(validate_signing_key("a\tb").is_err());
    }

    #[test]
    fn render_does_not_inject_a_section_via_control_chars() {
        // Even if a control-laden value reaches the renderer (validation is the
        // first line of defense), quoting/escaping must keep it on one line so
        // git reads it back verbatim and no extra section leaks in.
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("x.gitconfig");
        let id = Identity {
            name: "x".into(),
            user_name: "Jane".into(),
            email: "j@b.co".into(),
            signing_key: Some("KEY\n[core]\n\tsshCommand = touch PWNED".into()),
            sign: false,
            format: None,
            ssh_command: None,
        };
        atomic_write(&path, &render_fragment(&id)).unwrap();
        assert_eq!(
            gitcfg::get_file(&path, "user.signingkey")
                .unwrap()
                .as_deref(),
            Some("KEY\n[core]\n\tsshCommand = touch PWNED")
        );
        assert_eq!(gitcfg::get_file(&path, "core.sshCommand").unwrap(), None);
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
            format: None,
            ssh_command: None,
        };
        atomic_write(&path, &render_fragment(&id)).unwrap();
        assert_eq!(
            gitcfg::get_file(&path, "user.name").unwrap().as_deref(),
            Some("Quote \" Back\\slash #1")
        );
    }

    #[test]
    fn write_if_changed_skips_identical_content() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("f.txt");
        // Missing file -> writes.
        assert!(write_if_changed(&path, "one").unwrap());
        // Same content -> no write.
        assert!(!write_if_changed(&path, "one").unwrap());
        assert_eq!(fs::read_to_string(&path).unwrap(), "one");
        // Different content -> writes again.
        assert!(write_if_changed(&path, "two").unwrap());
        assert_eq!(fs::read_to_string(&path).unwrap(), "two");
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
