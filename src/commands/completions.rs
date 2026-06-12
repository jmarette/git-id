use std::process::ExitCode;

use anyhow::Result;
use clap::CommandFactory;
use clap_complete::{Shell, generate};
use clap_complete_nushell::Nushell;

use crate::cli::{Cli, CompletionShell, CompletionsArgs};

pub fn run(args: &CompletionsArgs) -> Result<ExitCode> {
    let mut cmd = Cli::command();
    let out = &mut std::io::stdout();
    match args.shell {
        CompletionShell::Bash => generate(Shell::Bash, &mut cmd, "git-id", out),
        CompletionShell::Elvish => generate(Shell::Elvish, &mut cmd, "git-id", out),
        CompletionShell::Fish => generate(Shell::Fish, &mut cmd, "git-id", out),
        CompletionShell::Nushell => generate(Nushell, &mut cmd, "git-id", out),
        CompletionShell::Powershell => generate(Shell::PowerShell, &mut cmd, "git-id", out),
        CompletionShell::Zsh => generate(Shell::Zsh, &mut cmd, "git-id", out),
    }
    Ok(ExitCode::SUCCESS)
}
