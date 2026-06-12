use std::process::ExitCode;

use anyhow::{Context, Result, anyhow, ensure};

use crate::cli::EditArgs;
use crate::env::Env;
use crate::store;

pub fn run(env: &Env, args: &EditArgs) -> Result<ExitCode> {
    ensure!(
        store::exists(env, &args.name),
        "identity `{}` does not exist (see `git-id list`)",
        args.name
    );
    let patch = store::IdentityPatch {
        user_name: args.user_name.clone(),
        email: args.email.clone(),
        signing_key: args.signing_key.clone(),
        sign: if args.sign {
            Some(true)
        } else if args.no_sign {
            Some(false)
        } else {
            None
        },
    };

    if !patch.is_empty() {
        // Targeted updates through `git config --file`: keys added to the
        // fragment by hand are preserved.
        store::apply_patch(env, &args.name, &patch)?;
        let id = store::load(env, &args.name)?;
        println!(
            "Updated identity `{}`: {} <{}>",
            id.name, id.user_name, id.email
        );
        return Ok(ExitCode::SUCCESS);
    }

    let path = store::fragment_path(env, &args.name);
    let editor = std::env::var("VISUAL")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .or_else(|| std::env::var("EDITOR").ok().filter(|v| !v.trim().is_empty()))
        .ok_or_else(|| {
            anyhow!("no flags given and neither $VISUAL nor $EDITOR is set — pass flags like --email, or set an editor")
        })?;
    let mut parts = editor.split_whitespace();
    let program = parts.next().expect("checked non-empty");
    let status = std::process::Command::new(program)
        .args(parts)
        .arg(&path)
        .status()
        .with_context(|| format!("failed to launch the editor `{editor}`"))?;
    ensure!(
        status.success(),
        "the editor `{editor}` exited with an error"
    );

    // The user may have broken the fragment; check now rather than letting
    // commits fail silently later.
    match store::load(env, &args.name) {
        Ok(id) => {
            println!(
                "Updated identity `{}`: {} <{}>",
                id.name, id.user_name, id.email
            );
            Ok(ExitCode::SUCCESS)
        }
        Err(e) => {
            eprintln!("warning: the identity fragment is now invalid: {e:#}");
            eprintln!(
                "fix it by re-running `git-id edit {}` ({})",
                args.name,
                path.display()
            );
            Ok(ExitCode::FAILURE)
        }
    }
}
