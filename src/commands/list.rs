use std::process::ExitCode;

use anyhow::Result;

use crate::cli::ListArgs;
use crate::env::Env;
use crate::output::{IdentityJson, UserJson, print_json};
use crate::paths::display_pretty;
use crate::{routes, store};

pub fn run(env: &Env, args: &ListArgs) -> Result<ExitCode> {
    let names = store::list_names(env)?;
    let model = routes::load(env)?;

    if args.paths {
        let mut entries: Vec<_> = model.entries.iter().collect();
        entries.sort_by(|a, b| a.gitdir.cmp(&b.gitdir));
        if entries.is_empty() {
            println!("No routes yet. Add one with `git-id use <name> [directory]`.");
            return Ok(ExitCode::SUCCESS);
        }
        for entry in entries {
            let who = match entry.identity() {
                Some(name) => name.to_string(),
                None => format!("(foreign include: {})", entry.target.display()),
            };
            println!("{}  ->  {}", display_pretty(&entry.gitdir, &env.home), who);
        }
        return Ok(ExitCode::SUCCESS);
    }

    if args.json {
        let mut out = Vec::new();
        for name in &names {
            match store::load(env, name) {
                Ok(id) => out.push(IdentityJson {
                    name: id.name,
                    path: store::fragment_path(env, name).display().to_string(),
                    user: UserJson {
                        name: id.user_name,
                        email: id.email,
                        signing_key: id.signing_key,
                        sign: id.sign,
                        format: id.format,
                        ssh_command: id.ssh_command,
                    },
                    routes: model
                        .gitdirs_for_identity(name)
                        .iter()
                        .map(|s| s.to_string())
                        .collect(),
                }),
                Err(e) => eprintln!("warning: skipping `{name}`: {e:#}"),
            }
        }
        print_json(&out)?;
        return Ok(ExitCode::SUCCESS);
    }

    if names.is_empty() {
        println!("No identities yet. Create one with `git-id create <name>`.");
        return Ok(ExitCode::SUCCESS);
    }
    for name in &names {
        match store::load(env, name) {
            Ok(id) => {
                let mut line = format!("{}  {} <{}>", id.name, id.user_name, id.email);
                if id.sign {
                    line.push_str("  [signs commits]");
                }
                println!("{line}");
            }
            Err(e) => println!("{name}  (invalid: {e:#})"),
        }
        let dirs = model.gitdirs_for_identity(name);
        if dirs.is_empty() {
            println!("    (no routes)");
        }
        for dir in dirs {
            println!("    -> {}", display_pretty(dir, &env.home));
        }
    }
    for entry in &model.entries {
        if let Some(id) = entry.identity() {
            if !names.iter().any(|n| n == id) {
                eprintln!(
                    "warning: {} is routed to `{id}`, but that identity no longer exists (run `git-id doctor`)",
                    display_pretty(&entry.gitdir, &env.home)
                );
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}
