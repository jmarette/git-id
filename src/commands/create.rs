use std::process::ExitCode;

use anyhow::{Result, bail};

use crate::cli::CreateArgs;
use crate::env::Env;
use crate::{prompt, store};

pub fn run(env: &Env, args: &CreateArgs) -> Result<ExitCode> {
    store::validate_slug(&args.name)?;
    if store::exists(env, &args.name) && !args.force {
        bail!(
            "identity `{0}` already exists — change it with `git-id edit {0}`, or pass --force to overwrite it",
            args.name
        );
    }

    // With no flags at all we run a full interactive session (including the
    // optional fields); otherwise only the missing required fields are
    // prompted for.
    let pure_interactive = args.user_name.is_none()
        && args.email.is_none()
        && args.signing_key.is_none()
        && !args.sign;

    let user_name = match &args.user_name {
        Some(v) => {
            store::validate_user_name(v)?;
            v.clone()
        }
        None => {
            require_tty("--name \"Full Name\"")?;
            prompt::ask("Full name", false, store::validate_user_name)?
        }
    };
    let email = match &args.email {
        Some(v) => {
            store::validate_email(v)?;
            v.clone()
        }
        None => {
            require_tty("--email you@example.com")?;
            prompt::ask("Email", false, store::validate_email)?
        }
    };
    let signing_key = match &args.signing_key {
        Some(v) if !v.is_empty() => {
            store::validate_signing_key(v)?;
            Some(v.clone())
        }
        Some(_) => None,
        None if pure_interactive => {
            let v = prompt::ask(
                "Signing key (user.signingkey)",
                true,
                store::validate_signing_key,
            )?;
            if v.is_empty() { None } else { Some(v) }
        }
        None => None,
    };
    let sign = args.sign
        || (pure_interactive
            && prompt::confirm("Sign commits by default (commit.gpgsign=true)?", false)?);

    let id = store::Identity {
        name: args.name.clone(),
        user_name,
        email,
        signing_key,
        sign,
    };
    store::write_new(env, &id, args.force)?;
    println!(
        "Created identity `{}`: {} <{}>",
        id.name, id.user_name, id.email
    );
    println!(
        "Route a directory to it with: git-id use {} <directory>",
        id.name
    );
    Ok(ExitCode::SUCCESS)
}

fn require_tty(flag: &str) -> Result<()> {
    if prompt::interactive() {
        Ok(())
    } else {
        bail!("missing required information and stdin is not a terminal — pass {flag}")
    }
}
