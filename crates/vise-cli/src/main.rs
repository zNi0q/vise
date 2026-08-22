//! The `vise` command-line driver.
//!
//! Argument parsing is hand-written, matching the workspace's
//! no-third-party-dependency policy.

use std::process::ExitCode;

use vise_diag::{Code, SourceMap, json, render};

const USAGE: &str = "\
vise — a language whose author is a machine

usage:
  vise lex <file.vise> [--json]   tokenise a file and report diagnostics
  vise explain <CODE>             explain a diagnostic code, e.g. V0401
  vise explain --list             list every diagnostic code
  vise help                       show this message
  vise version                    show the version
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();

    match refs.as_slice() {
        [] | ["help" | "-h" | "--help"] => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        ["version" | "-V" | "--version"] => {
            println!("vise {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        ["explain", "--list"] => {
            for code in Code::ALL {
                println!("{code}  {}", code.title());
            }
            ExitCode::SUCCESS
        }
        ["explain", code] => explain(code),
        ["lex", path] => lex_file(path, false),
        ["lex", path, "--json"] | ["lex", "--json", path] => lex_file(path, true),
        _ => {
            eprint!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn explain(text: &str) -> ExitCode {
    match text.parse::<Code>() {
        Ok(code) => {
            println!("{code}: {}\n\n{}", code.title(), code.explain());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}\nrun `vise explain --list` to see every code");
            ExitCode::from(2)
        }
    }
}

fn lex_file(path: &str, as_json: bool) -> ExitCode {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("cannot read {path}: {e}");
            return ExitCode::from(2);
        }
    };

    let mut map = SourceMap::new();
    let file = map.add(path, text.clone());
    let lexed = vise_lex::lex(&text, file);

    if as_json {
        println!("{}", json::report(&lexed.diagnostics, &map));
    } else {
        print!("{}", render::report(&lexed.diagnostics, &map));
        if lexed.diagnostics.is_empty() {
            println!("{} tokens, no diagnostics", lexed.without_newlines().len());
        }
    }

    if lexed.has_errors() {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
