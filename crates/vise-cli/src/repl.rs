//! An interactive session.
//!
//! Vise has no notion of a top level: a program is a module, and a module is a
//! set of declarations. So the REPL keeps a session — imports, declarations,
//! and the bindings made so far — and rebuilds a whole module around every
//! input.
//!
//! That works because of §11. The session re-runs from the start each time, and
//! a Vise program is deterministic, so everything the previous run printed is
//! reproduced identically and can simply be skipped. A REPL for a language
//! without that guarantee would have to do something cleverer.

use std::io::{BufRead, Write};
use std::process::ExitCode;

use vise_ast::{Binding, ItemKind, Stmt, StmtKind};
use vise_diag::{FileId, SourceMap, render};

const PROMPT: &str = "vise> ";
const CONTINUE: &str = "  ... ";

/// The name the REPL binds an evaluated expression to before printing it.
///
/// Binding first avoids nesting the expression inside a string literal, which
/// would break the moment it contained one of its own.
const IT: &str = "__it";

pub fn run() -> ExitCode {
    println!("vise {} — :help for commands", env!("CARGO_PKG_VERSION"));

    let mut session = Session::default();
    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines();
    let mut pending = String::new();

    loop {
        print(if pending.is_empty() { PROMPT } else { CONTINUE });
        let Some(line) = lines.next() else {
            println!();
            return ExitCode::SUCCESS;
        };
        let Ok(line) = line else {
            return ExitCode::FAILURE;
        };

        if pending.is_empty() && line.trim().is_empty() {
            continue;
        }
        if pending.is_empty() && line.trim_start().starts_with(':') {
            match session.command(line.trim()) {
                Command::Continue => {}
                Command::Quit => return ExitCode::SUCCESS,
            }
            continue;
        }

        pending.push_str(&line);
        pending.push('\n');
        // Keep reading while a bracket is still open, so a function definition
        // can span lines.
        if unclosed(&pending) > 0 {
            continue;
        }

        let input = std::mem::take(&mut pending);
        session.evaluate(input.trim());
    }
}

fn print(text: &str) {
    std::io::stdout().write_all(text.as_bytes()).ok();
    std::io::stdout().flush().ok();
}

/// Net open brackets, ignoring those inside strings and comments.
fn unclosed(text: &str) -> i32 {
    let mut depth = 0;
    for line in text.lines() {
        let mut in_string = false;
        let mut chars = line.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '\\' if in_string => {
                    chars.next();
                }
                '"' => in_string = !in_string,
                '-' if !in_string && chars.peek() == Some(&'-') => break,
                '(' | '[' | '{' if !in_string => depth += 1,
                ')' | ']' | '}' if !in_string => depth -= 1,
                _ => {}
            }
        }
    }
    depth
}

enum Command {
    Continue,
    Quit,
}

#[derive(Default)]
struct Session {
    uses: Vec<String>,
    items: Vec<String>,
    /// Bindings and assignments, replayed on every evaluation.
    stmts: Vec<String>,
    /// How many lines the replayed prelude prints, so they can be skipped.
    emitted: usize,
}

impl Session {
    /// Build a module from the session plus one more piece of input.
    fn source(&self, trailing: &str) -> String {
        let mut out = String::from("module repl\n");
        for u in &self.uses {
            out.push_str(u);
            out.push('\n');
        }
        for item in &self.items {
            out.push('\n');
            out.push_str(item);
            out.push('\n');
        }
        out.push_str("\nfn main() {\n");
        for s in &self.stmts {
            out.push_str(s);
            out.push('\n');
        }
        if !trailing.is_empty() {
            out.push_str(trailing);
            out.push('\n');
        }
        out.push_str("}\n");
        out
    }

    fn command(&mut self, line: &str) -> Command {
        let (name, argument) = line.split_once(' ').unwrap_or((line, ""));
        match name {
            ":quit" | ":q" | ":exit" => return Command::Quit,
            ":help" | ":h" => {
                println!(
                    "  :help          this message\n\
                     \x20 :quit          leave\n\
                     \x20 :list          show the session as a module\n\
                     \x20 :type <expr>   the type of an expression\n\
                     \x20 :reset         forget everything\n\
                     \x20 :save <file>   write the session to a file\n\
                     \n\
                     \x20 Declarations (fn, type, record, enum, use) are remembered.\n\
                     \x20 A `let` or `var` is remembered. Anything else is evaluated once."
                );
            }
            ":list" => print!("{}", self.source("")),
            ":reset" => {
                *self = Self::default();
                println!("session cleared");
            }
            ":type" => self.show_type(argument.trim()),
            ":save" => {
                let path = if argument.trim().is_empty() {
                    "session.vise"
                } else {
                    argument.trim()
                };
                match std::fs::write(path, self.source("")) {
                    Ok(()) => println!("wrote {path}"),
                    Err(e) => println!("cannot write {path}: {e}"),
                }
            }
            other => println!("unknown command {other}; :help lists them"),
        }
        Command::Continue
    }

    /// Report the inferred type of an expression without running anything.
    fn show_type(&self, expression: &str) {
        if expression.is_empty() {
            println!("usage: :type <expression>");
            return;
        }
        let source = self.source(&format!("let {IT} = {expression}"));
        let mut map = SourceMap::new();
        let file = map.add("<repl>", source.clone());
        let parsed = vise_parse::parse(&source, file);
        let Some(module) = &parsed.module else {
            print!("{}", render::report(&parsed.diagnostics, &map));
            return;
        };
        if parsed.has_errors() {
            print!("{}", render::report(&parsed.diagnostics, &map));
            return;
        }

        let (diagnostics, types) = vise_check::check_with_types(module);
        if let Some(span) = binding_value_span(module, IT)
            && let Some(ty) = types.get(&(span.start, span.end))
        {
            println!("{expression} : {ty}");
            return;
        }
        if diagnostics.is_empty() {
            println!("could not determine a type for that");
        } else {
            print!("{}", render::report(&diagnostics, &map));
        }
    }

    fn evaluate(&mut self, input: &str) {
        if input.is_empty() {
            return;
        }
        if is_declaration(input) {
            self.add_declaration(input);
            return;
        }

        // An expression with a value gets printed; `print("hi")` has type Unit
        // and must not be followed by a second line saying so.
        let trailing = match self.value_type(input) {
            Some(ty) if ty != "Unit" => format!("let {IT} = {input}\nprint(\"{{{IT}}}\")"),
            _ => input.to_owned(),
        };

        match self.attempt(&trailing, false) {
            Outcome::Ran(stdout) => {
                self.show(&stdout);
                // Whether to remember this is decided from the parsed
                // statement, not from what the text looks like.
                if self.persists(input) {
                    self.stmts.push(input.to_owned());
                    self.emitted = stdout.len();
                }
            }
            Outcome::Rejected(report) | Outcome::Trapped(report) => print!("{report}"),
        }
    }

    /// The type of `input` read as an expression, or `None` if it is not one.
    fn value_type(&self, input: &str) -> Option<String> {
        let source = self.source(&format!("let {IT} = {input}"));
        let module = parse_clean(&source)?;
        let (diagnostics, types) = vise_check::check_with_types(&module);
        if diagnostics.iter().any(vise_diag::Diagnostic::is_error) {
            return None;
        }
        let span = binding_value_span(&module, IT)?;
        types.get(&(span.start, span.end)).map(ToString::to_string)
    }

    /// Whether the statement changes something a later input could observe.
    fn persists(&self, input: &str) -> bool {
        let Some(module) = parse_clean(&self.source(input)) else {
            return false;
        };
        let Some(main) = module.items.iter().find_map(|i| match &i.kind {
            ItemKind::Fn(f) if f.name.name == "main" => Some(f),
            _ => None,
        }) else {
            return false;
        };
        // A trailing expression becomes the block's *tail*, not a statement.
        // Everything this session commits is a binding, an assignment, or a
        // loop, none of which can be a tail — so a tail means the input was an
        // expression, and an expression changes nothing for later inputs.
        if main.body.tail.is_some() {
            return false;
        }
        matches!(
            main.body.stmts.last().map(|s| &s.kind),
            Some(
                StmtKind::Let { .. }
                    | StmtKind::Assign { .. }
                    | StmtKind::For { .. }
                    | StmtKind::While { .. }
            )
        )
    }

    fn add_declaration(&mut self, input: &str) {
        let is_use = input.trim_start().starts_with("use ");
        let candidate = if is_use {
            let mut session = Session {
                uses: self.uses.clone(),
                ..Session::default()
            };
            session.uses.push(input.to_owned());
            session
        } else {
            let mut session = Session {
                uses: self.uses.clone(),
                items: self.items.clone(),
                ..Session::default()
            };
            session.items.push(input.to_owned());
            session
        };

        match candidate.attempt("", true) {
            Outcome::Ran(_) => {
                if is_use {
                    self.uses.push(input.to_owned());
                } else {
                    self.items.push(input.to_owned());
                }
                // A new declaration cannot change what the prelude prints, so
                // `emitted` stands.
            }
            Outcome::Rejected(report) | Outcome::Trapped(report) => print!("{report}"),
        }
    }

    /// Check, and unless `check_only`, run.
    fn attempt(&self, trailing: &str, check_only: bool) -> Outcome {
        let source = self.source(trailing);
        let mut map = SourceMap::new();
        let file = map.add("<repl>", source.clone());
        let parsed = vise_parse::parse(&source, file);

        let mut diagnostics = parsed.diagnostics;
        let module = match &parsed.module {
            Some(m) if !diagnostics.iter().any(|d| d.is_error()) => m,
            _ => return Outcome::Rejected(render::report(&diagnostics, &map)),
        };

        diagnostics.extend(vise_check::resolve(module));
        diagnostics.extend(vise_check::check_effects(module));
        diagnostics.extend(vise_check::check_exhaustive(module));
        diagnostics.extend(vise_check::check_results(module));
        diagnostics.extend(vise_check::check_types(module));
        diagnostics.extend(vise_check::check_borrows(module));
        if diagnostics.iter().any(|d| d.is_error()) {
            return Outcome::Rejected(render::report(&diagnostics, &map));
        }
        if check_only {
            return Outcome::Ran(Vec::new());
        }

        let outcome = vise_interp::run(module);
        match outcome.result {
            Ok(_) => Outcome::Ran(outcome.stdout),
            Err(trap) => {
                let mut text = String::new();
                for line in outcome.stdout.iter().skip(self.emitted) {
                    text.push_str(line);
                    text.push('\n');
                }
                text.push_str(&format!("trap: {trap}\n"));
                Outcome::Trapped(text)
            }
        }
    }

    /// Print only what this input added, skipping the replayed prelude.
    fn show(&self, stdout: &[String]) {
        for line in stdout.iter().skip(self.emitted) {
            println!("{line}");
        }
    }
}

enum Outcome {
    Ran(Vec<String>),
    Rejected(String),
    Trapped(String),
}

fn is_declaration(input: &str) -> bool {
    let head = input.trim_start();
    let head = head.strip_prefix("pub ").unwrap_or(head);
    ["fn ", "type ", "record ", "enum ", "use "]
        .iter()
        .any(|k| head.starts_with(k))
}

/// Parse, returning the module only when nothing went wrong.
fn parse_clean(source: &str) -> Option<vise_ast::Module> {
    let parsed = vise_parse::parse(source, FileId(0));
    if parsed.has_errors() {
        return None;
    }
    parsed.module
}

/// The span of the value bound by `let <name> = ...` in `main`.
fn binding_value_span(module: &vise_ast::Module, name: &str) -> Option<vise_diag::Span> {
    let main = module.items.iter().find_map(|i| match &i.kind {
        ItemKind::Fn(f) if f.name.name == "main" => Some(f),
        _ => None,
    })?;
    main.body.stmts.iter().rev().find_map(|s| match s {
        Stmt {
            kind:
                StmtKind::Let {
                    name: Binding::Name(ident),
                    value,
                    ..
                },
            ..
        } if ident.name == name => Some(value.span),
        _ => None,
    })
}
