use std::process::ExitCode;

use anyhow::Result;

use crate::cli::WhichArgs;
use crate::env::Env;
use crate::output::{EffectiveJson, WhichJson, print_json};
use crate::paths::{self, display_pretty};
use crate::{gitcfg, routes, store};

pub fn run(env: &Env, args: &WhichArgs) -> Result<ExitCode> {
    let input = args.path.clone().unwrap_or_else(|| env.cwd.clone());
    let nd = paths::normalize_dir(&input, env)?;
    let model = routes::load(env)?;
    let matched = model.longest_match(&nd.gitdir).cloned();

    // Predicted identity, from our route table + the fragment contents.
    let mut identity: Option<String> = None;
    let mut frag_name: Option<String> = None;
    let mut frag_email: Option<String> = None;
    if let Some(entry) = &matched {
        if let Some(name) = entry.identity() {
            identity = Some(name.to_string());
            match store::load(env, name) {
                Ok(id) => {
                    frag_name = Some(id.user_name);
                    frag_email = Some(id.email);
                }
                Err(e) => eprintln!("warning: {e:#}"),
            }
        }
    }

    // Ground truth from git, when the directory is inside a repository.
    let git_dir = if nd.existed {
        gitcfg::absolute_git_dir(&nd.path)?
    } else {
        None
    };
    let in_repo = git_dir.is_some();
    let mut effective: Option<EffectiveJson> = None;
    let mut mismatch = false;
    let mut notes: Vec<String> = Vec::new();

    if in_repo {
        let eff_name = gitcfg::effective(&nd.path, "user.name")?;
        let eff_email = gitcfg::effective_origin(&nd.path, "user.email")?;

        // Linked worktrees and submodules: git matches `gitdir:` against the
        // real .git directory, which lives under the *main* repository.
        if let (Some(gd), Some(top)) = (&git_dir, gitcfg::show_toplevel(&nd.path)?) {
            if !gd.starts_with(&top) {
                notes.push(format!(
                    "this work tree's git directory is {} — for linked worktrees and submodules, the identity follows the main repository's location, not this directory",
                    gd.display()
                ));
            }
        }

        match (&matched, &eff_email) {
            (Some(_), Some((origin, email))) => {
                if let Some(expected) = &frag_email {
                    if expected != email {
                        mismatch = true;
                        notes.push(format!(
                            "the effective identity here is {email} (from {origin}) — it overrides the matched route"
                        ));
                    }
                }
            }
            (Some(_), None) => {
                mismatch = true;
                notes.push(
                    "a route matches but git resolves no user.email here — run `git-id init` to make sure the routes file is included".to_string(),
                );
            }
            (None, _) => {}
        }

        effective = Some(EffectiveJson {
            name: eff_name,
            email: eff_email.as_ref().map(|(_, v)| v.clone()),
            origin: eff_email.as_ref().map(|(o, _)| o.clone()),
        });
    }

    if args.json {
        print_json(&WhichJson {
            identity: identity.clone(),
            gitdir: nd.gitdir.clone(),
            route: matched.as_ref().map(|e| e.gitdir.clone()),
            name: frag_name.clone(),
            email: frag_email.clone(),
            in_repo,
            effective,
            mismatch,
        })?;
        for note in &notes {
            eprintln!("warning: {note}");
        }
        return Ok(if identity.is_some() {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        });
    }

    let pretty_query = display_pretty(&nd.gitdir, &env.home);
    match &matched {
        Some(entry) => match &identity {
            Some(name) => {
                let who = match (&frag_name, &frag_email) {
                    (Some(n), Some(e)) => format!("{n} <{e}>"),
                    _ => "(fragment unreadable)".to_string(),
                };
                println!("{name} — {who}");
                println!("  route: {}", display_pretty(&entry.gitdir, &env.home));
                if entry.gitdir != nd.gitdir {
                    println!("  query: {pretty_query}");
                }
            }
            None => {
                println!(
                    "A foreign include matches {} -> {} (not a git-id identity).",
                    display_pretty(&entry.gitdir, &env.home),
                    entry.target.display()
                );
            }
        },
        None => {
            println!("No identity applies to {pretty_query}.");
            match &effective {
                Some(EffectiveJson {
                    email: Some(email),
                    origin: Some(origin),
                    ..
                }) => println!("Git falls back to {email} (from {origin})."),
                _ => println!("Route it with: git-id use <name> {pretty_query}"),
            }
        }
    }
    for note in &notes {
        eprintln!("warning: {note}");
    }
    Ok(if identity.is_some() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}
