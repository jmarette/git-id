use std::process::ExitCode;

use anyhow::{Result, ensure};

use crate::cli::UseArgs;
use crate::env::Env;
use crate::paths::{self, display_pretty};
use crate::{gitcfg, routes, store};

use super::init;

pub fn run(env: &Env, args: &UseArgs) -> Result<ExitCode> {
    // Load (not just an existence check) so a broken fragment is reported
    // now rather than at commit time.
    let id = store::load(env, &args.name)?;

    let input = args.path.clone().unwrap_or_else(|| env.cwd.clone());
    let nd = paths::normalize_dir(&input, env)?;
    ensure!(
        !paths::contains_glob_meta(&nd.gitdir),
        "`{}` contains glob characters (`*`, `?`, `[`) that git treats as pattern syntax in gitdir rules — rename the directory or route a parent",
        nd.gitdir
    );
    ensure!(
        !nd.gitdir.bytes().any(|b| b < 0x20),
        "`{}` contains control characters and cannot be routed",
        nd.gitdir.escape_default()
    );

    let pretty_dir = display_pretty(&nd.gitdir, &env.home);
    if !nd.existed {
        eprintln!(
            "warning: {pretty_dir} does not exist yet — the route is recorded and will apply once it is created"
        );
    } else {
        let git_dir = gitcfg::absolute_git_dir(&nd.path)?;
        let top = gitcfg::show_toplevel(&nd.path)?;
        // Linked worktrees and submodules: git matches `gitdir:` against the
        // real .git directory, which lives under the *main* repository — so a
        // route on this directory's own path is recorded but never applied.
        if let (Some(gd), Some(top)) = (&git_dir, &top) {
            if !gd.starts_with(top) {
                eprintln!(
                    "warning: {pretty_dir} is a linked worktree or submodule whose git directory is {} — git matches routes against the main repository's location, so this route will NOT apply here; route the main repository instead",
                    display_pretty(&gd.to_string_lossy(), &env.home)
                );
            }
        }
        if let Some(top) = &top {
            let top_slash = paths::ensure_trailing_slash(top.to_string_lossy().into_owned());
            if top_slash != nd.gitdir {
                eprintln!(
                    "note: {pretty_dir} is inside the repository {} — the route affects repositories under it, not that repository itself",
                    display_pretty(&top_slash, &env.home)
                );
            }
        }
    }

    let mut model = routes::load(env)?;
    let previous = model
        .entries
        .iter()
        .find(|e| e.gitdir == nd.gitdir)
        .cloned();
    if let Some(prev) = &previous {
        if prev.identity() == Some(args.name.as_str()) {
            println!("{pretty_dir} is already routed to `{}`.", args.name);
            return Ok(ExitCode::SUCCESS);
        }
    }
    model.set_route(&nd.gitdir, store::fragment_path(env, &args.name), env);
    routes::save(env, &model)?;

    match &previous {
        Some(prev) => {
            let old = prev
                .identity()
                .map(str::to_string)
                .unwrap_or_else(|| prev.target.display().to_string());
            println!("Routed {pretty_dir} -> {} (replaced `{old}`).", args.name);
        }
        None => {
            println!(
                "Routed {pretty_dir} -> {} ({} <{}>).",
                args.name, id.user_name, id.email
            );
        }
    }

    if !init::include_is_installed(env)? {
        eprintln!(
            "note: the global git config does not include git-id's routes yet — run `git-id init` to activate routing"
        );
    }
    Ok(ExitCode::SUCCESS)
}
