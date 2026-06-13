use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

/// Manage Git identities and route them to directories.
///
/// Identities are stored as gitconfig fragments under
/// `$XDG_CONFIG_HOME/git-id/` (default `~/.config/git-id/`) and applied per
/// directory through Git's native conditional includes
/// (`includeIf "gitdir:..."`), so the right user.name/user.email is picked
/// automatically wherever you clone.
#[derive(Parser)]
#[command(
    name = "git-id",
    version,
    about,
    subcommand_required = true,
    arg_required_else_help = true,
    after_help = "Quickstart:\n  \
        git id init\n  \
        git id create work --name \"Jane Doe\" --email jane@work.example\n  \
        git id use work ~/dev/work\n  \
        git id which"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Cmd,
}

#[derive(Subcommand)]
pub enum Cmd {
    /// Set up git-id (one time): link the routing file into the global git config
    Init(InitArgs),
    /// Create a new identity
    Create(CreateArgs),
    /// List identities and the directories routed to them
    List(ListArgs),
    /// Show one identity in detail
    Show(ShowArgs),
    /// Edit an identity (with flags, or in $EDITOR when no flags are given)
    Edit(EditArgs),
    /// Delete an identity and remove all routes pointing to it
    Delete(DeleteArgs),
    /// Route a directory (and everything below it) to an identity
    #[command(name = "use")]
    Use(UseArgs),
    /// Remove the route of a directory
    Unset(UnsetArgs),
    /// Show which identity applies to a directory
    #[command(visible_alias = "current")]
    Which(WhichArgs),
    /// Check the git-id setup for problems
    Doctor,
    /// Remove everything git-id set up (run before uninstalling the binary)
    Uninstall(UninstallArgs),
    /// Print shell completions, or install them with `completions install`
    #[command(args_conflicts_with_subcommands = true)]
    Completions(CompletionsArgs),
}

#[derive(Args)]
pub struct InitArgs {
    /// Also set `user.useConfigOnly=true` globally, so git refuses to commit
    /// when no identity applies to the current directory
    #[arg(long, conflicts_with = "no_use_config_only")]
    pub use_config_only: bool,
    /// Do not set `user.useConfigOnly` (skips the interactive question)
    #[arg(long)]
    pub no_use_config_only: bool,
}

#[derive(Args)]
pub struct CreateArgs {
    /// Identity name: a lowercase slug like `work` or `personal`
    pub name: String,
    /// Full name used in commits (user.name); prompted for if omitted
    #[arg(long = "name", value_name = "FULL_NAME")]
    pub user_name: Option<String>,
    /// Email used in commits (user.email); prompted for if omitted
    #[arg(long, value_name = "EMAIL")]
    pub email: Option<String>,
    /// Signing key (user.signingkey), e.g. a GPG key id
    #[arg(long, value_name = "KEY")]
    pub signing_key: Option<String>,
    /// Sign commits by default (sets commit.gpgsign=true)
    #[arg(long)]
    pub sign: bool,
    /// Overwrite the identity if it already exists
    #[arg(long)]
    pub force: bool,
}

#[derive(Args)]
pub struct ListArgs {
    /// Only show the directory -> identity mapping
    #[arg(long, conflicts_with = "json")]
    pub paths: bool,
    /// Machine-readable JSON output
    #[arg(long)]
    pub json: bool,
}

#[derive(Args)]
pub struct ShowArgs {
    /// Identity name
    pub name: String,
    /// Machine-readable JSON output
    #[arg(long)]
    pub json: bool,
}

#[derive(Args)]
pub struct EditArgs {
    /// Identity name
    pub name: String,
    /// New full name (user.name)
    #[arg(long = "name", value_name = "FULL_NAME")]
    pub user_name: Option<String>,
    /// New email (user.email)
    #[arg(long, value_name = "EMAIL")]
    pub email: Option<String>,
    /// New signing key (user.signingkey); pass an empty string to remove it
    #[arg(long, value_name = "KEY")]
    pub signing_key: Option<String>,
    /// Sign commits by default (sets commit.gpgsign=true)
    #[arg(long, conflicts_with = "no_sign")]
    pub sign: bool,
    /// Do not sign commits by default (sets commit.gpgsign=false)
    #[arg(long)]
    pub no_sign: bool,
}

#[derive(Args)]
pub struct DeleteArgs {
    /// Identity name
    pub name: String,
    /// Delete without asking for confirmation
    #[arg(long, visible_alias = "yes")]
    pub force: bool,
}

#[derive(Args)]
pub struct UseArgs {
    /// Identity name (must exist; see `git-id create`)
    pub name: String,
    /// Directory to route (default: current directory)
    pub path: Option<PathBuf>,
}

#[derive(Args)]
pub struct UnsetArgs {
    /// Directory whose route to remove (default: current directory)
    pub path: Option<PathBuf>,
}

#[derive(Args)]
pub struct UninstallArgs {
    /// Remove without asking for confirmation
    #[arg(long, visible_alias = "yes")]
    pub force: bool,
}

#[derive(Args)]
pub struct WhichArgs {
    /// Directory to inspect (default: current directory)
    pub path: Option<PathBuf>,
    /// Machine-readable JSON output
    #[arg(long)]
    pub json: bool,
}

/// Shells supported by `git-id completions`: the ones natively covered by
/// clap_complete, plus Nushell via clap_complete_nushell.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum CompletionShell {
    Bash,
    Elvish,
    Fish,
    Nushell,
    Powershell,
    Zsh,
}

#[derive(Args)]
pub struct CompletionsArgs {
    #[command(subcommand)]
    pub action: Option<CompletionsAction>,
    /// Shell to print completions for; auto-detected from $SHELL when omitted
    #[arg(value_enum)]
    pub shell: Option<CompletionShell>,
}

#[derive(Subcommand)]
pub enum CompletionsAction {
    /// Write the completion script into the right location for the shell,
    /// printing any one-time activation step
    Install {
        /// Shell to target; auto-detected from $SHELL when omitted
        #[arg(value_enum)]
        shell: Option<CompletionShell>,
    },
}
