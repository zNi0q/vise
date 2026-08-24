//! Builds the C runtime.
//!
//! `cc` is invoked directly rather than through the `cc` crate, so the
//! workspace's no-third-party-dependency policy holds for the build as well as
//! for the code.

use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let runtime = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../runtime/c");
    let asm = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../runtime/asm");
    for file in [
        "capability.c",
        "capability.h",
        "test_capability.c",
        "fiber.c",
        "fiber.h",
        "test_fiber.c",
    ] {
        println!("cargo:rerun-if-changed={}", runtime.join(file).display());
    }
    println!(
        "cargo:rerun-if-changed={}",
        asm.join("switch_x86_64.S").display()
    );
    println!("cargo:rerun-if-changed=build.rs");
    // Declare the cfg this script may set, so an unexpected-cfg warning cannot
    // hide a typo in it.
    println!("cargo:rustc-check-cfg=cfg(vise_no_runtime)");

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

    for source in ["capability.c", "fiber.c"] {
        let object = out.join(format!("{source}.o"));
        run(
            Command::new(&cc)
                .args([
                    "-std=c11", "-O2", "-fPIC", "-Wall", "-Wextra", "-Werror", "-c",
                ])
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

    // The C test binary is built here too, so a Rust test can run it. Seccomp
    // filters are irreversible, so the cases have to fork, which is far more
    // natural in C than through a Rust FFI shim.
    let caps_test = out.join("test_capability");
    run(
        Command::new(&cc)
            .args(["-std=c11", "-O2", "-Wall", "-Wextra", "-Werror"])
            .arg(runtime.join("capability.c"))
            .arg(runtime.join("test_capability.c"))
            .arg("-I")
            .arg(&runtime)
            .arg("-o")
            .arg(&caps_test),
        "building test_capability",
    );

    let fiber_test = out.join("test_fiber");
    let mut build_fibers = Command::new(&cc);
    build_fibers
        .args(["-std=c11", "-O2", "-Wall", "-Wextra", "-Werror"])
        .arg(runtime.join("fiber.c"))
        .arg(runtime.join("test_fiber.c"));
    if has_asm {
        build_fibers.arg(asm.join("switch_x86_64.S"));
    }
    run(
        build_fibers
            .arg("-I")
            .arg(&runtime)
            .arg("-o")
            .arg(&fiber_test),
        "building test_fiber",
    );

    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=static=vise_runtime");
    println!("cargo:rustc-env=VISE_CAPTEST={}", caps_test.display());
    println!("cargo:rustc-env=VISE_FIBERTEST={}", fiber_test.display());
}

fn run(command: &mut Command, what: &str) {
    let status = command
        .status()
        .unwrap_or_else(|e| panic!("{what}: could not start: {e}"));
    assert!(status.success(), "{what}: exited with {status}");
    let _: &Path = Path::new("");
}
