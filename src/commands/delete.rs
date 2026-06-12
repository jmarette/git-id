use std::process::ExitCode;

use anyhow::{Result, bail};

use crate::cli::DeleteArgs;
use crate::env::Env;
use crate::paths::display_pretty;
use crate::{prompt, routes, store};

pub fn run(env: &Env, args: &DeleteArgs) -> Result<ExitCode> {
    let fragment_exists = store::exists(env, &args.name);
    let mut model = routes::load(env)?;
    let dirs: Vec<String> = model
        .gitdirs_for_identity(&args.name)
        .iter()
        .map(|s| s.to_string())
        .collect();

    if !fragment_exists && dirs.is_empty() {
        bail!(
            "identity `{}` does not exist (see `git-id list`)",
            args.name
        );
    }

    if !args.force {
        eprintln!("About to delete identity `{}`:", args.name);
        if fragment_exists {
            eprintln!("  - {}", store::fragment_path(env, &args.name).display());
        }
        for dir in &dirs {
            eprintln!("  - route {}", display_pretty(dir, &env.home));
        }
        if !prompt::interactive() {
            bail!("refusing to delete without confirmation — pass --force in non-interactive use");
        }
        if !prompt::confirm("Proceed?", false)? {
            println!("Aborted.");
            return Ok(ExitCode::FAILURE);
        }
    }

    if fragment_exists {
        store::remove(env, &args.name)?;
    }
    let removed = model.remove_identity(&args.name);
    if !removed.is_empty() {
        routes::save(env, &model)?;
    }
    if removed.is_empty() {
        println!("Deleted identity `{}`.", args.name);
    } else {
        println!(
            "Deleted identity `{}` and {} route(s).",
            args.name,
            removed.len()
        );
    }
    Ok(ExitCode::SUCCESS)
}
