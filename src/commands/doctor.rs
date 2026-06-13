use std::collections::HashMap;
use std::path::Path;
use std::process::ExitCode;

use anyhow::Result;

use crate::env::Env;
use crate::paths::{self, display_pretty};
use crate::{gitcfg, routes, store};

use super::init;

#[derive(Default)]
struct Doctor {
    errors: u32,
    warnings: u32,
}

impl Doctor {
    fn error(&mut self, msg: &str) {
        self.errors += 1;
        println!("error:   {msg}");
    }
    fn warn(&mut self, msg: &str) {
        self.warnings += 1;
        println!("warning: {msg}");
    }
    fn info(&self, msg: &str) {
        println!("info:    {msg}");
    }
    fn ok(&self, msg: &str) {
        println!("ok:      {msg}");
    }
}

pub fn run(env: &Env) -> Result<ExitCode> {
    let mut d = Doctor::default();

    let (major, minor, patch) = gitcfg::git_version()?;
    if (major, minor) < (2, 13) {
        d.error(&format!(
            "git {major}.{minor}.{patch} is too old — conditional includes need git >= 2.13"
        ));
    } else {
        d.ok(&format!(
            "git {major}.{minor}.{patch} supports conditional includes"
        ));
    }

    let routes_pretty = display_pretty(&env.routes_file.to_string_lossy(), &env.home);
    if !env.routes_file.exists() {
        d.warn(&format!(
            "the routes file {routes_pretty} does not exist — run `git-id init`"
        ));
    }
    if init::include_is_installed(env)? {
        d.ok("the global git config includes the routes file");
    } else {
        d.error(&format!(
            "the global git config does not include {routes_pretty} — run `git-id init`"
        ));
    }

    let model = routes::load(env)?;
    for block in &model.preserved {
        let first_line = block.lines().next().unwrap_or("");
        d.warn(&format!(
            "routes.gitconfig contains content not managed by git-id (kept as-is): `{first_line}`"
        ));
    }

    let mut counts: HashMap<&str, u32> = HashMap::new();
    for entry in &model.entries {
        *counts.entry(entry.gitdir.as_str()).or_insert(0) += 1;
    }
    for (gitdir, n) in counts {
        if n > 1 {
            d.error(&format!(
                "{n} routes exist for {gitdir} — run `git-id use` on it again to deduplicate"
            ));
        }
    }

    let names = store::list_names(env)?;
    for entry in &model.entries {
        let pretty = display_pretty(&entry.gitdir, &env.home);
        match entry.identity() {
            Some(name) => match store::load(env, name) {
                Ok(id) => {
                    if let Err(err) = store::validate_email(&id.email) {
                        d.warn(&format!("identity `{name}`: {err:#}"));
                    }
                }
                Err(err) => d.error(&format!("route {pretty}: {err:#}")),
            },
            None => {
                d.warn(&format!(
                    "route {pretty} points outside git-id's identities: {}",
                    entry.target.display()
                ));
                if !entry.target.exists() {
                    d.warn(&format!(
                        "the include target {} does not exist (git silently ignores it)",
                        entry.target.display()
                    ));
                }
            }
        }

        if !Path::new(&entry.gitdir).exists() {
            d.warn(&format!(
                "the routed directory {pretty} does not exist (the route stays dormant)"
            ));
        } else {
            let (canon, _) =
                paths::canonicalize_anchored(Path::new(entry.gitdir.trim_end_matches('/')));
            let canon_slash = paths::ensure_trailing_slash(canon.to_string_lossy().into_owned());
            if canon_slash != entry.gitdir {
                d.warn(&format!(
                    "route {pretty} differs from the directory's canonical path {canon_slash} — git matches canonical paths, re-run `git-id use` on it"
                ));
            }
        }
        if paths::contains_glob_meta(&entry.gitdir) {
            d.warn(&format!(
                "route {pretty} contains glob characters (`*`, `?`, `[`) that git treats as pattern syntax"
            ));
        }
        if !entry.gitdir.is_ascii() {
            d.info(&format!(
                "route {pretty} contains non-ASCII characters; Unicode normalization differences can break matching on macOS"
            ));
        }
    }

    for name in &names {
        if store::validate_slug(name).is_err() {
            d.warn(&format!(
                "identity file `{name}.gitconfig` does not follow the naming rules (lowercase slug)"
            ));
        }
        if let Err(err) = store::load(env, name) {
            d.error(&format!("{err:#}"));
        }
        if model.gitdirs_for_identity(name).is_empty() {
            d.info(&format!(
                "identity `{name}` has no routes (route it with `git-id use {name} <dir>`)"
            ));
        }
    }

    if init::useconfigonly_is_enabled(env)? {
        d.ok("user.useConfigOnly is enabled — commits require an explicit identity");
    } else {
        d.info(
            "user.useConfigOnly is not enabled — git may guess an identity where no route matches (enable with `git-id init --use-config-only`)",
        );
    }

    println!();
    if d.errors == 0 && d.warnings == 0 {
        println!("doctor: everything looks good");
    } else {
        println!("doctor: {} error(s), {} warning(s)", d.errors, d.warnings);
    }
    Ok(if d.errors == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}
