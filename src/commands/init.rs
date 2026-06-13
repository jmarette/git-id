use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};

use crate::cli::InitArgs;
use crate::env::Env;
use crate::paths::display_pretty;
use crate::{gitcfg, paths, prompt, routes};

/// The global-level config files git actually reads and that git-id may have
/// written to, in increasing precedence order, existing files only. Mirrors
/// git: an explicit `GIT_CONFIG_GLOBAL` is the *only* global file; otherwise
/// git reads BOTH `$XDG_CONFIG_HOME/git/config` and `~/.gitconfig` (the latter
/// wins). We scan these directly rather than `git config --global`, because
/// `--global` resolves to a single file (`~/.gitconfig` once it exists) and so
/// goes blind to an include we legitimately wrote into the XDG file before
/// `~/.gitconfig` came to exist.
pub fn global_config_files(env: &Env) -> Vec<PathBuf> {
    let candidates = if let Some(global) = &env.git_config_global {
        vec![global.clone()]
    } else {
        vec![
            env.config_base.join("git").join("config"),
            env.home.join(".gitconfig"),
        ]
    };
    candidates.into_iter().filter(|p| p.exists()).collect()
}

/// Whether the global git config already includes our routes file.
pub fn include_is_installed(env: &Env) -> Result<bool> {
    // Compare in git-path form so a path Git normalized to forward slashes on
    // Windows still matches what we wrote (and stays byte-identical on Unix).
    let want = paths::to_git_path(routes_path_str(env)?);
    for file in global_config_files(env) {
        for p in gitcfg::get_file_all(&file, "include.path")? {
            if paths::to_git_path(&p) == want
                || paths::expand_tilde(&p, &env.home) == env.routes_file
            {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// Whether `user.useConfigOnly` resolves to true across the global config
/// files (later files win, mirroring git's precedence).
pub fn useconfigonly_is_enabled(env: &Env) -> Result<bool> {
    let mut enabled = false;
    for file in global_config_files(env) {
        if let Some(value) = gitcfg::get_file_bool(&file, "user.useConfigOnly")? {
            enabled = value;
        }
    }
    Ok(enabled)
}

pub fn routes_path_str(env: &Env) -> Result<&str> {
    env.routes_file
        .to_str()
        .context("the git-id config path is not valid UTF-8")
}

/// At most one timestamped backup of the global config per mutating run,
/// taken right before the first write — and never on no-op re-runs.
struct Backup {
    done: bool,
    path: Option<PathBuf>,
}

impl Backup {
    fn ensure(&mut self, env: &Env) -> Result<()> {
        if self.done {
            return Ok(());
        }
        self.done = true;
        let (target, exists) = env.global_config_write_target();
        if !exists {
            return Ok(());
        }
        let name = target
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("gitconfig");
        // The timestamp has 1-second resolution, so find a free name rather than
        // letting fs::copy silently clobber a same-second backup.
        let ts = timestamp_utc()?;
        let mut bak = target.with_file_name(format!("{name}.bak-{ts}"));
        let mut n = 1;
        while bak.exists() {
            bak = target.with_file_name(format!("{name}.bak-{ts}-{n}"));
            n += 1;
        }
        fs::copy(&target, &bak)
            .with_context(|| format!("cannot back up {} to {}", target.display(), bak.display()))?;
        self.path = Some(bak);
        Ok(())
    }
}

pub fn run(env: &Env, args: &InitArgs) -> Result<ExitCode> {
    fs::create_dir_all(&env.identities_dir)
        .with_context(|| format!("cannot create {}", env.identities_dir.display()))?;

    let created_routes = if env.routes_file.exists() {
        false
    } else {
        routes::save(env, &routes::Routes::default())?;
        true
    };

    let mut backup = Backup {
        done: false,
        path: None,
    };

    let mut added_include = false;
    if !include_is_installed(env)? {
        backup.ensure(env)?;
        gitcfg::global_add("include.path", &paths::to_git_path(routes_path_str(env)?))?;
        added_include = true;
    }

    enum Uco {
        Enabled,
        AlreadyEnabled,
        Declined,
        SkippedNonInteractive,
    }
    let already_on = useconfigonly_is_enabled(env)?;
    let uco = if already_on {
        Uco::AlreadyEnabled
    } else {
        let wanted = if args.use_config_only {
            Some(true)
        } else if args.no_use_config_only {
            Some(false)
        } else if prompt::interactive() {
            Some(prompt::confirm(
                "Set user.useConfigOnly=true, so git refuses to commit where no identity applies?",
                true,
            )?)
        } else {
            None
        };
        match wanted {
            Some(true) => {
                backup.ensure(env)?;
                gitcfg::global_set("user.useConfigOnly", "true")?;
                Uco::Enabled
            }
            Some(false) => Uco::Declined,
            None => Uco::SkippedNonInteractive,
        }
    };

    let pretty = |p: &std::path::Path| display_pretty(&p.to_string_lossy(), &env.home);
    println!("git-id config directory: {}", pretty(&env.config_dir));
    if created_routes {
        println!("Created the routes file: {}", pretty(&env.routes_file));
    }
    if added_include {
        let (target, _) = env.global_config_write_target();
        println!(
            "Linked the routes file into the global git config: {}",
            pretty(&target)
        );
    } else {
        println!("The global git config already includes the routes file.");
    }
    if let Some(bak) = &backup.path {
        println!("Backed up the previous global config to: {}", pretty(bak));
    }
    match uco {
        Uco::Enabled => {
            println!(
                "Enabled user.useConfigOnly: git now refuses to commit where no identity applies."
            );
        }
        Uco::AlreadyEnabled => println!("user.useConfigOnly is already enabled."),
        Uco::Declined => {}
        Uco::SkippedNonInteractive => {
            println!(
                "Skipped user.useConfigOnly (non-interactive). Enable it later with `git-id init --use-config-only`."
            );
        }
    }
    println!();
    println!("Next steps:");
    println!("  git-id create <name> --name \"Full Name\" --email you@example.com");
    println!("  git-id use <name> [directory]");
    Ok(ExitCode::SUCCESS)
}

fn timestamp_utc() -> Result<String> {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("the system clock is set before 1970")?
        .as_secs();
    Ok(format_timestamp(secs))
}

/// `YYYYMMDD-HHMMSS` in UTC, computed with the standard civil-from-days
/// algorithm (no date dependency needed for a backup suffix).
fn format_timestamp(unix_secs: u64) -> String {
    let days = (unix_secs / 86_400) as i64;
    let rem = unix_secs % 86_400;
    let (hh, mm, ss) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let mut y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    if m <= 2 {
        y += 1;
    }
    format!("{y:04}{m:02}{d:02}-{hh:02}{mm:02}{ss:02}")
}

#[cfg(test)]
mod tests {
    use super::format_timestamp;

    #[test]
    fn timestamp_formatting() {
        assert_eq!(format_timestamp(0), "19700101-000000");
        // Leap day 2000.
        assert_eq!(format_timestamp(951_782_400), "20000229-000000");
        // Well-known round value: 2023-11-14 22:13:20 UTC.
        assert_eq!(format_timestamp(1_700_000_000), "20231114-221320");
    }
}
