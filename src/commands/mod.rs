pub mod completions;
pub mod create;
pub mod delete;
pub mod doctor;
pub mod edit;
pub mod init;
pub mod list;
pub mod show;
pub mod unset;
pub mod r#use;
pub mod which;

use std::process::ExitCode;

use anyhow::Result;

use crate::cli::Cmd;
use crate::env::Env;

pub fn dispatch(env: &Env, cmd: &Cmd) -> Result<ExitCode> {
    match cmd {
        Cmd::Init(args) => init::run(env, args),
        Cmd::Create(args) => create::run(env, args),
        Cmd::List(args) => list::run(env, args),
        Cmd::Show(args) => show::run(env, args),
        Cmd::Edit(args) => edit::run(env, args),
        Cmd::Delete(args) => delete::run(env, args),
        Cmd::Use(args) => r#use::run(env, args),
        Cmd::Unset(args) => unset::run(env, args),
        Cmd::Which(args) => which::run(env, args),
        Cmd::Doctor => doctor::run(env),
        Cmd::Completions(args) => completions::run(args),
    }
}
