# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.1]

### Changed

- CI: bump `actions/checkout` from v4 to v6 and `Swatinem/rust-cache` from v2
  to v2.9.1 (both now run on Node.js 24, ahead of GitHub's June 2026 forced
  migration).

## [0.3.0]

### Added

- Identities can carry per-directory signing and SSH settings. `create`/`edit`
  gain `--format <openpgp|ssh|x509>` (`gpg.format`, for SSH commit signing on
  git ≥ 2.34), `--ssh-key <path>` — shorthand for
  `core.sshCommand = ssh -i <path> -o IdentitiesOnly=yes`, so a per-identity key
  is used and an agent key cannot shadow it — and `--ssh-command <cmd>` to store
  a full `core.sshCommand` verbatim. `edit --no-format` / `--no-ssh` remove them.
  Both new values are surfaced in `show`/`list --json` (`user.format`,
  `user.ssh_command`) and are rejected if they carry control characters.
- `doctor` flags an identity whose `gpg.format` is not one of openpgp/ssh/x509,
  and warns when `gpg.format = ssh` but the installed git is older than 2.34
  (which cannot sign with SSH).
- A man page. `git-id man` prints it (rendered from the CLI definition, like the
  shell completions), and `git-id init` installs it beside the binary — in
  `<prefix>/share/man/man1` for a cargo/installer/Homebrew layout, or
  `~/.local/share/man` as a fallback — so `man git-id` and `git id --help`
  (which git rewrites to `man git-id`) work with no extra step. `git-id
  uninstall` removes it; `doctor` reports whether it is installed. No-op on
  Windows, which has no man pages.

### Fixed

- `doctor` no longer reports every routed directory as differing from its
  canonical path on Windows (it now compares in the same forward-slash,
  de-UNC'd form routes are stored in).
- `create`/`edit` reject control characters in a signing key, and rendered
  fragments escape control characters, closing a gitconfig-section injection
  via a newline-bearing `--signing-key`.
- `init`, `doctor` and `uninstall` find the git-id include and
  `user.useConfigOnly` even when they live in `$XDG_CONFIG_HOME/git/config` and
  a `~/.gitconfig` is later created; `uninstall` now removes them from whichever
  global config file holds them.
- `use` warns when routing a linked worktree's or submodule's own path, where
  git matches against the main repository and the route would never apply.
- `edit` validates all fields before writing any, so a rejected edit no longer
  leaves an earlier field already changed.
- `delete` rewrites the routes file before removing the fragment, so an
  interruption cannot leave a route pointing at a deleted fragment.
- `doctor` also flags a duplicate `gitdir` route when one side is a hand-added
  (preserved) `[includeIf]` block.
- `init` no longer overwrites an earlier same-second backup of the global
  config.

## [0.2.0]

### Added

- `git-id uninstall` removes everything git-id set up — the `include.path`
  line in the global git config, the `user.useConfigOnly` guard, and the
  config directory — on any platform, so no shell-specific cleanup is needed
  before removing the binary. Pass `--yes` to skip the confirmation.
- `git-id completions install [shell]` sets up shell completions
  automatically: it detects your shell from `$SHELL` (or takes one
  explicitly), writes the completion script to the right location, and prints
  the single activation step for shells without a completion autoload
  directory (zsh, nushell, elvish, powershell). `git-id completions <shell>`
  still prints the script to stdout for manual setup.

## [0.1.0]

Initial release.

### Added

- `git-id init` — one-time, idempotent setup: links the generated routes file
  into the global git config with a single `include.path` line, takes a
  timestamped backup of the global config before any modification (honoring
  `GIT_CONFIG_GLOBAL` when set), and offers to set `user.useConfigOnly=true`
  so git refuses to commit where no identity applies.
- Identity management — `create`, `list`, `show`, `edit`, `delete`.
  Identities are plain gitconfig fragments stored under
  `${XDG_CONFIG_HOME:-~/.config}/git-id/identities/`; `edit` patches through
  `git config --file` so keys added by hand are preserved; `delete` also
  purges every route pointing at the identity.
- Directory routing — `use`, `unset`, built on Git's native
  `includeIf "gitdir:..."` conditional includes. Routed paths are stored in
  Git's canonical form (symlinks resolved, e.g. `/tmp` → `/private/tmp` on
  macOS; forward-slash and de-UNC'd on Windows) with a trailing slash, and
  rendered sorted parent-before-child so the deepest route always wins.
  Re-routing a directory replaces its route — no duplicates, ever.
- `which` (alias `current`) — reports the identity applying to a directory,
  the effective name/email git actually resolves from inside a repository,
  local-config overrides (`mismatch`), and the linked-worktree/submodule
  trap where the identity follows the main repository's location.
- `doctor` — integrity checks: include line present, fragments valid, stale
  or duplicate routes, glob metacharacters in routed paths, git version,
  `user.useConfigOnly` state.
- `--json` output on `list`, `show` and `which` for scripting; `which` exits
  non-zero when no identity applies.
- Shell completions for bash, zsh, fish, nushell, elvish and powershell
  (`git-id completions <shell>`).
- Native git subcommand integration: `git id <command>` ≡ `git-id <command>`.
- Cross-platform support for macOS, Linux and Windows: the home directory is
  resolved exactly as Git does on each platform, and CI runs the full suite
  on all three.
- Atomic writes (temp file + rename) for every file the tool owns; content
  not managed by git-id found in the routes file is preserved verbatim.
- Prebuilt binaries and shell/PowerShell installers for macOS, Linux and
  Windows on every release, built and published by
  [cargo-dist](https://axodotdev.github.io/cargo-dist/), plus a Homebrew
  formula pushed to the tap.
- Dual licensed under MIT OR Apache-2.0.

[Unreleased]: https://github.com/jmarette/git-id/compare/v0.3.1...HEAD
[0.3.1]: https://github.com/jmarette/git-id/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/jmarette/git-id/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/jmarette/git-id/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/jmarette/git-id/releases/tag/v0.1.0
