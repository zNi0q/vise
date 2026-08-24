//! The `vise` command-line driver.
//!
//! Argument parsing is hand-written, matching the workspace's
//! no-third-party-dependency policy.

mod repl;

use std::process::ExitCode;

use vise_diag::{Code, Diagnostic, SourceMap, json, render};

const USAGE: &str = "\
vise — a language whose author is a machine

usage:
  vise check <file.vise> [--json]  parse, resolve, and type-check
  vise repl                        start an interactive session
  vise run <file.vise>             check, then run `main`
  vise build <file.vise> [-o out]  check, then compile to a native binary
  vise fix <file.vise> [--dry-run] apply every unambiguous fix
  vise fmt <file.vise> [--check]   rewrite in canonical form
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
        ["repl"] => repl::run(),
        ["run", path] => run_file(path),
        ["build", path] => build_file(path, None),
        ["build", path, "-o", out] | ["build", "-o", out, path] => build_file(path, Some(out)),
        ["fmt", path] => fmt_file(path, false),
        ["fmt", path, "--check"] | ["fmt", "--check", path] => fmt_file(path, true),
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
            diagnostics.extend(vise_check::check_borrows(module));
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

/// The C runtime is embedded rather than looked up on disk, so a built binary
/// does not depend on where the compiler was installed from.
const VALUE_H: &str = include_str!("../../../runtime/c/value.h");
const VALUE_C: &str = include_str!("../../../runtime/c/value.c");

/// Check, lower to C, and hand the result to a C compiler.
fn build_file(path: &str, output: Option<&str>) -> ExitCode {
    let text = match read(path) {
        Ok(t) => t,
        Err(code) => return code,
    };

    let mut map = SourceMap::new();
    let file = map.add(path, text.clone());
    let parsed = vise_parse::parse(&text, file);
    let a = analyze(path, &text, Stage::Check);
    if a.errors() > 0 {
        print!("{}", render::report(&a.diagnostics, &a.map));
        eprintln!("not building: {} error(s)", a.errors());
        return ExitCode::FAILURE;
    }
    let Some(module) = &parsed.module else {
        return ExitCode::FAILURE;
    };

    // Re-run inference for its type map; the backend needs what each
    // expression turned out to be.
    let (_, types) = vise_check::check_with_types(module);
    let emitted = vise_codegen::emit(module, &types);
    if !emitted.is_complete() {
        print!("{}", render::report(&emitted.unsupported, &a.map));
        eprintln!(
            "not building: {} unsupported construct(s)",
            emitted.unsupported.len()
        );
        return ExitCode::FAILURE;
    }

    let stem = std::path::Path::new(path)
        .file_stem()
        .map_or_else(|| "a.out".to_owned(), |s| s.to_string_lossy().into_owned());
    let binary = output.map_or(stem, ToOwned::to_owned);

    let dir = std::env::temp_dir().join(format!("vise-build-{}", std::process::id()));
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("cannot create a build directory: {e}");
        return ExitCode::from(2);
    }

    let write = |name: &str, contents: &str| std::fs::write(dir.join(name), contents);
    if let Err(e) = write("value.h", VALUE_H)
        .and_then(|()| write("value.c", VALUE_C))
        .and_then(|()| write("program.c", &emitted.c_source))
    {
        eprintln!("cannot write the generated sources: {e}");
        return ExitCode::from(2);
    }

    let cc = std::env::var("CC").unwrap_or_else(|_| "cc".to_owned());
    let status = std::process::Command::new(&cc)
        .args(["-std=c11", "-O2", "-o"])
        .arg(&binary)
        .arg(dir.join("program.c"))
        .arg(dir.join("value.c"))
        .arg("-I")
        .arg(&dir)
        .status();

    let _ = std::fs::remove_dir_all(&dir);

    match status {
        Ok(s) if s.success() => {
            println!("built {binary}");
            ExitCode::SUCCESS
        }
        Ok(s) => {
            eprintln!("the C compiler exited with {s}");
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("could not run {cc}: {e}");
            ExitCode::from(2)
        }
    }
}

/// Rewrite a file in canonical form, or report that it is not.
fn fmt_file(path: &str, check_only: bool) -> ExitCode {
    let text = match read(path) {
        Ok(t) => t,
        Err(code) => return code,
    };

    let mut map = SourceMap::new();
    let file = map.add(path, text.clone());
    let parsed = vise_parse::parse(&text, file);

    // Formatting reprints the tree, so a file that does not parse cannot be
    // formatted without losing what the author wrote.
    if parsed.has_errors() || parsed.module.is_none() {
        print!("{}", render::report(&parsed.diagnostics, &map));
        eprintln!("not formatting: the file does not parse");
        return ExitCode::FAILURE;
    }
    let module = parsed.module.expect("checked above");
    let formatted = vise_fmt::format(&module);

    if formatted == text {
        if !check_only {
            println!("{path} is already canonical");
        }
        return ExitCode::SUCCESS;
    }
    if check_only {
        eprintln!("{path} is not canonical");
        return ExitCode::FAILURE;
    }
    if let Err(e) = std::fs::write(path, &formatted) {
        eprintln!("cannot write {path}: {e}");
        return ExitCode::from(2);
    }
    println!("formatted {path}");
    ExitCode::SUCCESS
}

/// Check, then execute `main`.
fn run_file(path: &str) -> ExitCode {
    let text = match read(path) {
        Ok(t) => t,
        Err(code) => return code,
    };

    let mut map = SourceMap::new();
    let file = map.add(path, text.clone());
    let parsed = vise_parse::parse(&text, file);
    let a = analyze(path, &text, Stage::Check);

    // Running code the checker rejected would report a runtime failure for
    // something already explained statically.
    if a.errors() > 0 {
        print!("{}", render::report(&a.diagnostics, &a.map));
        eprintln!("not running: {} error(s)", a.errors());
        return ExitCode::FAILURE;
    }
    print!("{}", render::report(&a.diagnostics, &a.map));

    let Some(module) = &parsed.module else {
        return ExitCode::FAILURE;
    };
    let outcome = vise_interp::run(module);
    // Print what ran before reporting how it ended.
    for line in &outcome.stdout {
        println!("{line}");
    }
    match outcome.result {
        Ok(value) => {
            if value != vise_interp::Value::Unit {
                println!("=> {value}");
            }
            ExitCode::SUCCESS
        }
        Err(trap) => {
            eprintln!("trap: {trap}");
            ExitCode::FAILURE
        }
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
