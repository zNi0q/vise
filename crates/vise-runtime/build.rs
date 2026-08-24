//! Builds the C runtime.
//!
//! `cc` is invoked directly rather than through the `cc` crate, so the
//! workspace's no-third-party-dependency policy holds for the build as well as
//! for the code.

use std::path::PathBuf;
use std::process::Command;

/// Every C translation unit in the runtime library.
const SOURCES: &[&str] = &["capability.c", "fiber.c", "trace.c", "softfloat.c"];

/// C test programs, and the library sources each needs. They are built here and
/// run from a Rust test: the cases fork, install irreversible filters, and
/// switch stacks, all of which are far more natural in C than through an FFI
/// shim.
const TEST_PROGRAMS: &[(&str, &[&str])] = &[
    ("test_capability", &["capability.c"]),
    ("test_fiber", &["fiber.c"]),
    ("test_trace", &["trace.c"]),
    ("test_softfloat", &["softfloat.c"]),
];

/// Warnings are errors: this code is small enough that every one is real.
const CFLAGS: &[&str] = &["-std=c11", "-O2", "-Wall", "-Wextra", "-Werror"];

fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let runtime = root.join("../../runtime/c");
    let asm = root.join("../../runtime/asm");

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rustc-check-cfg=cfg(vise_no_runtime)");
    for dir in [&runtime, &asm] {
        for entry in std::fs::read_dir(dir).expect("the runtime directories should exist") {
            println!(
                "cargo:rerun-if-changed={}",
                entry.expect("a readable entry").path().display()
            );
        }
    }

    // The gate is Linux-only; elsewhere the Rust side reports it unsupported.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("linux") {
        println!("cargo:rustc-cfg=vise_no_runtime");
        return;
    }

    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let cc = std::env::var("CC").unwrap_or_else(|_| "cc".to_owned());
    let ar = std::env::var("AR").unwrap_or_else(|_| "ar".to_owned());

    // The context switch exists only for x86-64. Elsewhere fiber.c compiles to
    // stubs that report themselves unsupported, rather than to something that
    // would appear to work.
    let has_asm = std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("x86_64");
    let mut objects = Vec::new();

    for source in SOURCES {
        let object = out.join(format!("{source}.o"));
        run(
            Command::new(&cc)
                .args(CFLAGS)
                .args(["-fPIC", "-c"])
                .arg(runtime.join(source))
                .arg("-I")
                .arg(&runtime)
                .arg("-o")
                .arg(&object),
            &format!("compiling {source}"),
        );
        objects.push(object);
    }

    if has_asm {
        let object = out.join("switch_x86_64.o");
        run(
            Command::new(&cc)
                .args(["-O2", "-c"])
                .arg(asm.join("switch_x86_64.S"))
                .arg("-o")
                .arg(&object),
            "assembling switch_x86_64.S",
        );
        objects.push(object);
    }

    let archive = out.join("libvise_runtime.a");
    let _ = std::fs::remove_file(&archive);
    run(
        Command::new(&ar).arg("rcs").arg(&archive).args(&objects),
        "archiving libvise_runtime.a",
    );

    for (name, sources) in TEST_PROGRAMS {
        let binary = out.join(name);
        let mut build = Command::new(&cc);
        build.args(CFLAGS);
        for source in *sources {
            build.arg(runtime.join(source));
        }
        build.arg(runtime.join(format!("{name}.c")));
        if *name == "test_fiber" && has_asm {
            build.arg(asm.join("switch_x86_64.S"));
        }
        // The softfloat suite compares against the platform libm, which is the
        // only place this project links it.
        if *name == "test_softfloat" {
            build.arg("-lm");
        }
        run(
            build.arg("-I").arg(&runtime).arg("-o").arg(&binary),
            &format!("building {name}"),
        );
        // Rust tests find each program through its own environment variable.
        println!(
            "cargo:rustc-env=VISE_{}={}",
            name.to_uppercase(),
            binary.display()
        );
    }

    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=static=vise_runtime");
}

fn run(command: &mut Command, what: &str) {
    let status = command
        .status()
        .unwrap_or_else(|e| panic!("{what}: could not start: {e}"));
    assert!(status.success(), "{what}: exited with {status}");
}
