use std::fs;
use std::process::ExitCode;

use anyhow::{Context, Result, bail};

use crate::cli::UninstallArgs;
use crate::env::Env;
use crate::paths::display_pretty;
use crate::{gitcfg, prompt};

use super::init;

/// Matches the git-id include entry by the tail of its path, so it is found
/// whatever the absolute prefix or path style (forward slashes, `~`, a custom
/// `XDG_CONFIG_HOME`).
const ROUTES_INCLUDE_REGEX: &str = r"git-id/routes\.gitconfig$";

pub fn run(env: &Env, args: &UninstallArgs) -> Result<ExitCode> {
    let include = init::include_is_installed(env)?;
    let dir_exists = env.config_dir.exists();
    let use_config_only = init::useconfigonly_is_enabled(env)?;

    if !include && !dir_exists && !use_config_only {
        println!("git-id is not set up — nothing to remove.");
        print_binary_hint();
        return Ok(ExitCode::SUCCESS);
    }

    let pretty = display_pretty(&env.config_dir.to_string_lossy(), &env.home);
    if !args.force {
        eprintln!("This will remove git-id's setup:");
        if dir_exists {
            eprintln!("  - {pretty} (all identities and routes)");
        }
        if include {
            eprintln!("  - the git-id include line in your global git config");
        }
        if use_config_only {
            eprintln!("  - user.useConfigOnly (so git can commit again without a git-id route)");
        }
        if !prompt::interactive() {
            bail!("refusing to uninstall without confirmation — pass --yes in non-interactive use");
        }
        if !prompt::confirm("Proceed?", false)? {
            println!("Aborted.");
            return Ok(ExitCode::FAILURE);
        }
    }

    // Clean every global config file git reads, not just the one `--global`
    // resolves to: the include (or useConfigOnly) may live in the XDG file even
    // when ~/.gitconfig now exists, and `--global` would never reach it.
    for file in init::global_config_files(env) {
        if include {
            gitcfg::unset_all_matching_file(&file, "include.path", ROUTES_INCLUDE_REGEX)?;
        }
        if use_config_only {
            gitcfg::unset_file(&file, "user.useConfigOnly")?;
        }
    }
    if dir_exists {
        fs::remove_dir_all(&env.config_dir)
            .with_context(|| format!("cannot remove {}", env.config_dir.display()))?;
    }

    println!("Removed git-id's configuration.");
    print_binary_hint();
    Ok(ExitCode::SUCCESS)
}

fn print_binary_hint() {
    println!("To remove the git-id binary, use the method you installed it with:");
    println!("  Homebrew:             brew uninstall git-id");
    println!("  cargo install:        cargo uninstall git-id");
    println!("  standalone installer: delete the git-id binary from ~/.cargo/bin");
}
