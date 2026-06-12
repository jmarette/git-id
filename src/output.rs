//! Pinned JSON shapes — this is the machine API exposed by `--json` and
//! asserted by the integration tests. Field names are stable; extend, don't
//! rename.

use anyhow::Result;
use serde::Serialize;

#[derive(Serialize)]
pub struct UserJson {
    pub name: String,
    pub email: String,
    pub signing_key: Option<String>,
    pub sign: bool,
}

/// One element of `git-id list --json`; also the shape of `show --json`.
#[derive(Serialize)]
pub struct IdentityJson {
    /// Identity slug.
    pub name: String,
    /// Fragment file path.
    pub path: String,
    pub user: UserJson,
    /// Directories routed to this identity (absolute, trailing slash).
    pub routes: Vec<String>,
}

#[derive(Serialize)]
pub struct EffectiveJson {
    pub name: Option<String>,
    pub email: Option<String>,
    /// File the effective user.email comes from.
    pub origin: Option<String>,
}

#[derive(Serialize)]
pub struct WhichJson {
    /// Matched identity slug, if a managed route applies.
    pub identity: Option<String>,
    /// The queried directory (absolute, trailing slash).
    pub gitdir: String,
    /// The matched route's directory, if any.
    pub route: Option<String>,
    /// user.name from the identity fragment.
    pub name: Option<String>,
    /// user.email from the identity fragment.
    pub email: Option<String>,
    /// Whether the queried directory is inside a git repository.
    pub in_repo: bool,
    /// What git actually resolves from inside the repository.
    pub effective: Option<EffectiveJson>,
    /// True when git resolves something different from the matched route
    /// (local override, missing include line, ...).
    pub mismatch: bool,
}

pub fn print_json<T: Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}
