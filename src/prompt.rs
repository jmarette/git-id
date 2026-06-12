//! Minimal interactive prompts. Prompts are written to stderr so stdout
//! stays clean for actual output, and every entry point fails fast with an
//! actionable error when stdin is not a terminal (scripts and CI must never
//! hang on a hidden prompt).

use std::io::{IsTerminal, Write};

use anyhow::{Context, Result, bail};

pub fn interactive() -> bool {
    std::io::stdin().is_terminal()
}

fn read_line() -> Result<String> {
    let mut buf = String::new();
    let n = std::io::stdin()
        .read_line(&mut buf)
        .context("failed to read from stdin")?;
    if n == 0 {
        bail!("unexpected end of input");
    }
    Ok(buf.trim().to_string())
}

/// Ask for a value; loops until `validate` accepts it.
/// An empty answer is allowed only when `allow_empty` is set (returns `""`).
pub fn ask(
    label: &str,
    allow_empty: bool,
    validate: impl Fn(&str) -> Result<()>,
) -> Result<String> {
    if !interactive() {
        bail!("stdin is not a terminal; cannot prompt for {label}");
    }
    loop {
        let suffix = if allow_empty { " (optional)" } else { "" };
        eprint!("{label}{suffix}: ");
        std::io::stderr().flush().ok();
        let answer = read_line()?;
        if answer.is_empty() && allow_empty {
            return Ok(answer);
        }
        match validate(&answer) {
            Ok(()) => return Ok(answer),
            Err(e) => eprintln!("{e}"),
        }
    }
}

pub fn confirm(question: &str, default: bool) -> Result<bool> {
    if !interactive() {
        bail!("stdin is not a terminal; cannot ask for confirmation");
    }
    let hint = if default { "[Y/n]" } else { "[y/N]" };
    loop {
        eprint!("{question} {hint} ");
        std::io::stderr().flush().ok();
        match read_line()?.to_lowercase().as_str() {
            "" => return Ok(default),
            "y" | "yes" => return Ok(true),
            "n" | "no" => return Ok(false),
            _ => eprintln!("please answer `y` or `n`"),
        }
    }
}
