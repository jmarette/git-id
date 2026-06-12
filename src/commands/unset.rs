use std::process::ExitCode;

use anyhow::Result;

use crate::cli::UnsetArgs;
use crate::env::Env;
use crate::paths::{self, display_pretty};
use crate::routes;

pub fn run(env: &Env, args: &UnsetArgs) -> Result<ExitCode> {
    let input = args.path.clone().unwrap_or_else(|| env.cwd.clone());
    let nd = paths::normalize_dir(&input, env)?;
    let pretty_dir = display_pretty(&nd.gitdir, &env.home);

    let mut model = routes::load(env)?;
    if model.remove_gitdir(&nd.gitdir) {
        routes::save(env, &model)?;
        println!("Removed the route for {pretty_dir}.");
    } else {
        println!("No route for {pretty_dir} — nothing to do.");
        if let Some(parent) = model.longest_match(&nd.gitdir) {
            let who = parent.identity().unwrap_or("a foreign include");
            println!(
                "note: {} -> {who} still covers this directory; unset that path to remove it.",
                display_pretty(&parent.gitdir, &env.home)
            );
        }
    }
    Ok(ExitCode::SUCCESS)
}
