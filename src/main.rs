mod cli;
mod commands;
mod env;
mod gitcfg;
mod output;
mod paths;
mod prompt;
mod routes;
mod store;

use std::process::ExitCode;

use clap::Parser;

fn main() -> ExitCode {
    let cli = cli::Cli::parse();
    // Completions need no environment; keep them usable even without HOME.
    if let cli::Cmd::Completions(args) = &cli.command {
        return match commands::completions::run(args) {
            Ok(code) => code,
            Err(err) => fail(&err),
        };
    }
    let env = match env::Env::from_process() {
        Ok(env) => env,
        Err(err) => return fail(&err),
    };
    match commands::dispatch(&env, &cli.command) {
        Ok(code) => code,
        Err(err) => fail(&err),
    }
}

fn fail(err: &anyhow::Error) -> ExitCode {
    eprintln!("error: {err:#}");
    ExitCode::FAILURE
}
