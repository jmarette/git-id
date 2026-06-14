use std::process::ExitCode;

use anyhow::{Context, Result};

use crate::cli::ShowArgs;
use crate::env::Env;
use crate::output::{IdentityJson, UserJson, print_json};
use crate::paths::display_pretty;
use crate::{routes, store};

pub fn run(env: &Env, args: &ShowArgs) -> Result<ExitCode> {
    let id = store::load(env, &args.name)?;
    let model = routes::load(env)?;
    let dirs: Vec<String> = model
        .gitdirs_for_identity(&args.name)
        .iter()
        .map(|s| s.to_string())
        .collect();
    let path = store::fragment_path(env, &args.name);

    if args.json {
        print_json(&IdentityJson {
            name: id.name,
            path: path.display().to_string(),
            user: UserJson {
                name: id.user_name,
                email: id.email,
                signing_key: id.signing_key,
                sign: id.sign,
                format: id.format,
                ssh_command: id.ssh_command,
            },
            routes: dirs,
        })?;
        return Ok(ExitCode::SUCCESS);
    }

    println!("identity: {}", id.name);
    println!(
        "file:     {}",
        display_pretty(&path.to_string_lossy(), &env.home)
    );
    println!("name:     {}", id.user_name);
    println!("email:    {}", id.email);
    if let Some(key) = &id.signing_key {
        println!(
            "signing:  {key}{}",
            if id.sign {
                " (commit.gpgsign=true)"
            } else {
                ""
            }
        );
    } else if id.sign {
        println!("signing:  commit.gpgsign=true");
    }
    if let Some(format) = &id.format {
        println!("format:   {format} (gpg.format)");
    }
    if let Some(cmd) = &id.ssh_command {
        println!("ssh:      {cmd}");
    }
    if dirs.is_empty() {
        println!("routes:   (none)");
    } else {
        println!("routes:");
        for dir in &dirs {
            println!("  -> {}", display_pretty(dir, &env.home));
        }
    }
    println!();
    println!("# {}", path.display());
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("cannot read {}", path.display()))?;
    print!("{content}");
    Ok(ExitCode::SUCCESS)
}
