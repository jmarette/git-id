use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Result, bail};
use clap::{CommandFactory, ValueEnum};
use clap_complete::{Shell, generate};
use clap_complete_nushell::Nushell;

use crate::cli::{Cli, CompletionShell, CompletionsAction, CompletionsArgs};
use crate::env::Env;
use crate::paths::display_pretty;
use crate::store;

/// Render the completion script for `shell` as a UTF-8 string.
fn render(shell: CompletionShell) -> String {
    let mut cmd = Cli::command();
    let mut buf = Vec::new();
    match shell {
        CompletionShell::Bash => generate(Shell::Bash, &mut cmd, "git-id", &mut buf),
        CompletionShell::Elvish => generate(Shell::Elvish, &mut cmd, "git-id", &mut buf),
        CompletionShell::Fish => generate(Shell::Fish, &mut cmd, "git-id", &mut buf),
        CompletionShell::Nushell => generate(Nushell, &mut cmd, "git-id", &mut buf),
        CompletionShell::Powershell => generate(Shell::PowerShell, &mut cmd, "git-id", &mut buf),
        CompletionShell::Zsh => generate(Shell::Zsh, &mut cmd, "git-id", &mut buf),
    }
    String::from_utf8(buf).expect("clap_complete emits UTF-8")
}

fn shell_name(shell: CompletionShell) -> String {
    shell
        .to_possible_value()
        .expect("no CompletionShell variant is skipped")
        .get_name()
        .to_string()
}

/// Map a shell's executable path (typically `$SHELL`) to a known shell.
fn detect_shell(shell_path: Option<&str>) -> Option<CompletionShell> {
    let path = shell_path?;
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let name = name.strip_suffix(".exe").unwrap_or(name);
    Some(match name {
        "bash" => CompletionShell::Bash,
        "zsh" => CompletionShell::Zsh,
        "fish" => CompletionShell::Fish,
        "nu" | "nushell" => CompletionShell::Nushell,
        "elvish" => CompletionShell::Elvish,
        "pwsh" | "powershell" => CompletionShell::Powershell,
        _ => return None,
    })
}

/// The explicit shell, or one detected from `$SHELL`.
fn resolve_shell(explicit: Option<CompletionShell>) -> Result<CompletionShell> {
    if let Some(shell) = explicit {
        return Ok(shell);
    }
    if let Some(shell) = detect_shell(std::env::var("SHELL").ok().as_deref()) {
        return Ok(shell);
    }
    bail!(
        "could not detect your shell from $SHELL; name it explicitly, e.g. \
         `git-id completions install zsh` (bash, zsh, fish, nushell, elvish, powershell)"
    )
}

struct InstallTarget {
    path: PathBuf,
    /// A one-time activation step, for shells with no autoload directory.
    activation: Option<String>,
}

/// Where each shell's completion file goes, and whether the user must wire it
/// in by hand (shells without an autoload directory).
fn completion_target(
    shell: CompletionShell,
    home: &Path,
    config_base: &Path,
    data_base: &Path,
) -> InstallTarget {
    match shell {
        // bash-completion and fish both autoload from these directories.
        CompletionShell::Bash => InstallTarget {
            path: data_base.join("bash-completion/completions/git-id"),
            activation: None,
        },
        CompletionShell::Fish => InstallTarget {
            path: config_base.join("fish/completions/git-id.fish"),
            activation: None,
        },
        // zsh autoloads from any directory on $fpath; ~/.zfunc is a common one.
        CompletionShell::Zsh => InstallTarget {
            path: home.join(".zfunc/_git-id"),
            activation: Some(
                "ensure ~/.zfunc is on your fpath: add `fpath+=(~/.zfunc)` before `compinit` in ~/.zshrc"
                    .to_string(),
            ),
        },
        // Nushell, Elvish and PowerShell have no completion autoload dir: the
        // generated file must be sourced from the shell's startup file.
        CompletionShell::Nushell => {
            let path = config_base.join("nushell/completions/git-id.nu");
            let activation = Some(format!("add `source \"{}\"` to your config.nu", path.display()));
            InstallTarget { path, activation }
        }
        CompletionShell::Elvish => {
            let path = config_base.join("elvish/lib/git-id.elv");
            let activation = Some(format!("add `eval (slurp < {})` to your rc.elv", path.display()));
            InstallTarget { path, activation }
        }
        CompletionShell::Powershell => {
            let path = config_base.join("powershell/git-id.ps1");
            let activation = Some(format!("dot-source it from your $PROFILE: `. {}`", path.display()));
            InstallTarget { path, activation }
        }
    }
}

/// Print the completion script to stdout. Needs no filesystem or HOME, so it
/// stays usable in minimal build/packaging environments.
pub fn print(shell: Option<CompletionShell>) -> Result<ExitCode> {
    let shell = resolve_shell(shell)?;
    let mut cmd = Cli::command();
    let out = &mut io::stdout();
    match shell {
        CompletionShell::Bash => generate(Shell::Bash, &mut cmd, "git-id", out),
        CompletionShell::Elvish => generate(Shell::Elvish, &mut cmd, "git-id", out),
        CompletionShell::Fish => generate(Shell::Fish, &mut cmd, "git-id", out),
        CompletionShell::Nushell => generate(Nushell, &mut cmd, "git-id", out),
        CompletionShell::Powershell => generate(Shell::PowerShell, &mut cmd, "git-id", out),
        CompletionShell::Zsh => generate(Shell::Zsh, &mut cmd, "git-id", out),
    }
    Ok(ExitCode::SUCCESS)
}

/// Write the completion script to the right place for the shell.
pub fn install(env: &Env, shell: Option<CompletionShell>) -> Result<ExitCode> {
    let shell = resolve_shell(shell)?;
    let target = completion_target(shell, &env.home, &env.config_base, &env.data_base());
    store::atomic_write(&target.path, &render(shell))?;

    let pretty = display_pretty(&target.path.to_string_lossy(), &env.home);
    println!("Installed {} completions to {pretty}", shell_name(shell));
    match &target.activation {
        Some(step) => println!("To activate them, {step}, then restart your shell."),
        None => println!("Restart your shell (or open a new session) to use them."),
    }
    Ok(ExitCode::SUCCESS)
}

pub fn run(env: &Env, args: &CompletionsArgs) -> Result<ExitCode> {
    match &args.action {
        Some(CompletionsAction::Install { shell }) => install(env, *shell),
        None => print(args.shell),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_shell_from_executable_path() {
        assert_eq!(detect_shell(Some("/bin/bash")), Some(CompletionShell::Bash));
        assert_eq!(detect_shell(Some("/bin/zsh")), Some(CompletionShell::Zsh));
        assert_eq!(
            detect_shell(Some("/usr/local/bin/fish")),
            Some(CompletionShell::Fish)
        );
        assert_eq!(
            detect_shell(Some("/opt/homebrew/bin/nu")),
            Some(CompletionShell::Nushell)
        );
        assert_eq!(
            detect_shell(Some("C:\\Program Files\\PowerShell\\7\\pwsh.exe")),
            Some(CompletionShell::Powershell)
        );
        assert_eq!(detect_shell(Some("/usr/bin/tcsh")), None);
        assert_eq!(detect_shell(None), None);
    }

    #[test]
    fn targets_use_xdg_dirs_and_flag_manual_shells() {
        let home = Path::new("/home/jane");
        let cfg = Path::new("/home/jane/.config");
        let data = Path::new("/home/jane/.local/share");

        let bash = completion_target(CompletionShell::Bash, home, cfg, data);
        assert_eq!(
            bash.path,
            PathBuf::from("/home/jane/.local/share/bash-completion/completions/git-id")
        );
        assert!(bash.activation.is_none());

        let fish = completion_target(CompletionShell::Fish, home, cfg, data);
        assert_eq!(
            fish.path,
            PathBuf::from("/home/jane/.config/fish/completions/git-id.fish")
        );
        assert!(fish.activation.is_none());

        let zsh = completion_target(CompletionShell::Zsh, home, cfg, data);
        assert_eq!(zsh.path, PathBuf::from("/home/jane/.zfunc/_git-id"));
        assert!(zsh.activation.is_some());

        let nu = completion_target(CompletionShell::Nushell, home, cfg, data);
        assert_eq!(
            nu.path,
            PathBuf::from("/home/jane/.config/nushell/completions/git-id.nu")
        );
        assert!(nu.activation.unwrap().contains("config.nu"));
    }

    #[test]
    fn render_emits_a_script_naming_the_binary() {
        assert!(render(CompletionShell::Bash).contains("git-id"));
        assert!(render(CompletionShell::Nushell).contains("git-id"));
    }
}
