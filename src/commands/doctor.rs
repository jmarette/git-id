use std::collections::HashMap;
use std::path::Path;
use std::process::ExitCode;

use anyhow::Result;

use crate::env::Env;
use crate::paths::{self, display_pretty};
use crate::{gitcfg, routes, store};

use super::completions::{self, CompletionStatus};
use super::{init, man};

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

    // Hand-added `[includeIf "gitdir:..."]` blocks live in `preserved`; count
    // them alongside managed entries so a duplicate split across a managed
    // route and a preserved block (git applies both) is still flagged.
    let preserved_gitdirs: Vec<String> = model
        .preserved
        .iter()
        .filter_map(|block| block.lines().next())
        .filter_map(routes::parse_gitdir_header)
        .collect();
    let mut counts: HashMap<&str, u32> = HashMap::new();
    for entry in &model.entries {
        *counts.entry(entry.gitdir.as_str()).or_insert(0) += 1;
    }
    for gitdir in &preserved_gitdirs {
        *counts.entry(gitdir.as_str()).or_insert(0) += 1;
    }
    for (gitdir, n) in counts {
        if n > 1 {
            d.error(&format!(
                "{n} routes exist for {gitdir} — git applies all of them (last wins); remove the duplicate `[includeIf]` block(s) from routes.gitconfig"
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
            // Compare in git-path form (forward slashes, de-UNC'd) so the stored
            // gitdir is not falsely flagged as non-canonical on Windows, where
            // fs::canonicalize yields a `\\?\C:\...` path. Identity on Unix.
            let canon_slash =
                paths::ensure_trailing_slash(paths::to_git_path(&canon.to_string_lossy()));
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
        match store::load(env, name) {
            Ok(id) => {
                if let Some(format) = &id.format {
                    // A hand-edited fragment can hold a bogus `gpg.format`; git
                    // would only fail at signing time.
                    if let Err(err) = store::validate_format(format) {
                        d.warn(&format!("identity `{name}`: {err:#}"));
                    } else if format == "ssh" && (major, minor) < (2, 34) {
                        d.warn(&format!(
                            "identity `{name}` uses gpg.format=ssh, which needs git >= 2.34 to sign (you have {major}.{minor}.{patch})"
                        ));
                    }
                }
            }
            Err(err) => d.error(&format!("{err:#}")),
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

    // On Windows there is no man system; `installed_man_path` returns None and
    // we say nothing. Elsewhere, report whether the page `init` installs is in
    // place (discoverability by `man` still depends on the manpath).
    match man::installed_man_path(env) {
        Some(path) if path.exists() => d.ok(&format!(
            "man page installed: {}",
            display_pretty(&path.to_string_lossy(), &env.home)
        )),
        Some(path) => d.info(&format!(
            "no man page at {} — run `git-id init` to install it (or `git-id man` to print it)",
            display_pretty(&path.to_string_lossy(), &env.home)
        )),
        None => {}
    }

    // For each shell on PATH, report whether git-id's completion file is in
    // place and current. This only inspects the file git-id writes — it cannot
    // see whether the activation line is wired into the shell's rc file, so for
    // manual-activation shells we add a reminder rather than claim it is active.
    for state in completions::completion_status(env) {
        let name = completions::shell_display_name(state.shell);
        let path = display_pretty(&state.path.to_string_lossy(), &env.home);
        match state.status {
            CompletionStatus::Installed => {
                if state.needs_activation {
                    d.ok(&format!(
                        "{name} completion installed: {path} (ensure its activation line is in your shell rc)"
                    ));
                } else {
                    d.ok(&format!("{name} completion installed: {path}"));
                }
            }
            CompletionStatus::Stale => d.info(&format!(
                "{name} completion at {path} is out of date — run `git-id completions install`"
            )),
            CompletionStatus::Missing => d.info(&format!(
                "{name} completion not installed — run `git-id completions install`"
            )),
        }
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
