//! The `vise` command-line driver.
//!
//! Argument parsing is hand-written, matching the workspace's
//! no-third-party-dependency policy.

use std::process::ExitCode;

use vise_diag::{Code, Diagnostic, SourceMap, json, render};

const USAGE: &str = "\
vise — a language whose author is a machine

usage:
  vise check <file.vise> [--json]  parse, resolve, and type-check
  vise fix <file.vise> [--dry-run] apply every unambiguous fix
  vise parse <file.vise> [--json]  parse only
  vise lex <file.vise> [--json]    tokenise only
  vise explain <CODE>              explain a diagnostic code, e.g. V0401
  vise explain --list              list every diagnostic code
  vise help                        show this message
  vise version                     show the version
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
        ["parse", path] => run(path, Stage::Parse, false),
        ["parse", path, "--json"] | ["parse", "--json", path] => run(path, Stage::Parse, true),
        ["check", path] => run(path, Stage::Check, false),
        ["check", path, "--json"] | ["check", "--json", path] => run(path, Stage::Check, true),
        ["fix", path] => fix_file(path, false),
        ["fix", path, "--dry-run"] | ["fix", "--dry-run", path] => fix_file(path, true),
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

fn read(path: &str) -> Result<String, ExitCode> {
    std::fs::read_to_string(path).map_err(|e| {
        eprintln!("cannot read {path}: {e}");
        ExitCode::from(2)
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    Parse,
    Check,
}

struct Analysis {
    map: SourceMap,
    diagnostics: Vec<Diagnostic>,
    /// A one-line description of what was accepted, when nothing failed.
    summary: Option<String>,
}

impl Analysis {
    fn errors(&self) -> usize {
        self.diagnostics.iter().filter(|d| d.is_error()).count()
    }
}

/// Run the compiler over `text` up to `stage`.
fn analyze(path: &str, text: &str, stage: Stage) -> Analysis {
    let mut map = SourceMap::new();
    let file = map.add(path, text.to_owned());
    let parsed = vise_parse::parse(text, file);
    let mut diagnostics = parsed.diagnostics;
    let mut summary = None;

    if let Some(module) = &parsed.module {
        // Later stages assume a tree; running them on a broken parse would
        // bury the real error under cascading noise.
        if stage == Stage::Check {
            let lines = map.file(file).line_count();
            if let Some(d) = vise_check::check_module_length(lines, module.name.span) {
                diagnostics.push(d);
            }
            diagnostics.extend(vise_check::resolve(module));
            diagnostics.extend(vise_check::check_effects(module));
            diagnostics.extend(vise_check::check_exhaustive(module));
            diagnostics.extend(vise_check::check_results(module));
            diagnostics.extend(vise_check::check_types(module));
        }
        summary = Some(format!(
            "module {}: {} import(s), {} item(s)",
            module.name,
            module.uses.len(),
            module.items.len()
        ));
    }

    Analysis {
        map,
        diagnostics,
        summary,
    }
}

fn run(path: &str, stage: Stage, as_json: bool) -> ExitCode {
    let text = match read(path) {
        Ok(t) => t,
        Err(code) => return code,
    };
    let a = analyze(path, &text, stage);

    if as_json {
        println!("{}", json::report(&a.diagnostics, &a.map));
    } else {
        print!("{}", render::report(&a.diagnostics, &a.map));
        if a.errors() == 0
            && let Some(s) = &a.summary
        {
            let verdict = if stage == Stage::Check {
                ", checks pass"
            } else {
                ""
            };
            println!("{s}{verdict}");
        }
    }

    if a.errors() > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn fix_file(path: &str, dry_run: bool) -> ExitCode {
    let text = match read(path) {
        Ok(t) => t,
        Err(code) => return code,
    };

    let before = analyze(path, &text, Stage::Check);
    let result = vise_diag::apply(&text, &before.diagnostics);

    if !result.changed() {
        print!("{}", render::report(&before.diagnostics, &before.map));
        println!("nothing to fix automatically");
        return if before.errors() > 0 {
            ExitCode::FAILURE
        } else {
            ExitCode::SUCCESS
        };
    }

    if !dry_run && let Err(e) = std::fs::write(path, &result.text) {
        eprintln!("cannot write {path}: {e}");
        return ExitCode::from(2);
    }

    // Report against the rewritten source, so what is left refers to what the
    // file now says rather than to what it used to.
    let after = analyze(path, &result.text, Stage::Check);
    print!("{}", render::report(&after.diagnostics, &after.map));

    let verb = if dry_run { "would apply" } else { "applied" };
    let plural = if result.applied == 1 { "" } else { "es" };
    println!("{verb} {} fix{plural}", result.applied);
    if result.skipped > 0 {
        println!("{} skipped for overlapping an applied edit", result.skipped);
    }
    if after.errors() > 0 {
        println!("{} error(s) need a human decision", after.errors());
    }

    if after.errors() > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn lex_file(path: &str, as_json: bool) -> ExitCode {
    let text = match read(path) {
        Ok(t) => t,
        Err(code) => return code,
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
