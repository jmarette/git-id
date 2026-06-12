# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] — 2026-06-13

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

[0.1.0]: https://github.com/jmarette/git-id/releases/tag/v0.1.0
