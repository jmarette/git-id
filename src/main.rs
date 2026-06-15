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
    // Printing completions needs no environment (no HOME) — keep it usable in
    // minimal build/packaging setups. Installing them does need paths, so it
    // falls through to the normal env-backed dispatch below.
    if let cli::Cmd::Completions(args) = &cli.command {
        if args.action.is_none() {
            return match commands::completions::print(args.shell) {
                Ok(code) => code,
                Err(err) => fail(&err),
            };
        }
    }
    // Rendering the man page is likewise HOME-free (a packager may run it in a
    // minimal sandbox); installing it happens through `init`, which has an Env.
    if let cli::Cmd::Man = &cli.command {
        return match commands::man::print() {
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
