//! The capability gate.
//!
//! Confinement itself is exercised by `runtime/c/test_capability.c`, which is
//! built alongside this crate and run below. Seccomp filters are irreversible,
//! so every case has to fork, which is far more natural in C than through an
//! FFI shim.

use vise_ast::Effect;
use vise_runtime::{bit, bits, confine, syscall_count};

#[test]
fn effect_bits_follow_the_declaration_order() {
    // A silent drift between this, `Effect`, and `capability.h` would confine a
    // program to the wrong syscall set, which is the one bug this layer exists
    // to prevent.
    for (i, effect) in Effect::ALL.iter().enumerate() {
        assert_eq!(bit(*effect), 1 << i, "{}", effect.as_str());
    }
}

#[test]
fn a_row_is_the_union_of_its_effects() {
    assert_eq!(bits(&[]), 0);
    assert_eq!(bits(&[Effect::Io]), 1);
    assert_eq!(bits(&[Effect::Io, Effect::Net]), 0b101);
    // Order and repetition do not matter.
    assert_eq!(
        bits(&[Effect::Net, Effect::Io, Effect::Net]),
        bits(&[Effect::Io, Effect::Net])
    );
    assert_eq!(bits(Effect::ALL), 0x7f);
}

#[test]
#[cfg(target_os = "linux")]
fn widening_a_row_only_ever_adds_syscalls() {
    let pure = syscall_count(&[]);
    assert!(pure > 0, "a pure program must still be able to exit");

    for effect in Effect::ALL {
        let widened = syscall_count(&[*effect]);
        assert!(
            widened >= pure,
            "`{}` removed syscalls from the base set",
            effect.as_str()
        );
    }
    assert!(syscall_count(Effect::ALL) > syscall_count(&[Effect::Net]));
}

#[test]
#[cfg(target_os = "linux")]
fn env_grants_no_syscall() {
    // The environment arrives on the initial stack, so reading it never enters
    // the kernel. `env` is a compiler-only effect by nature.
    assert_eq!(syscall_count(&[Effect::Env]), syscall_count(&[]));
}

/// Runs the C suite, which forks for each case and inspects how the child died.
#[test]
#[cfg(target_os = "linux")]
fn the_gate_actually_confines() {
    let binary = env!("VISE_CAPTEST");
    let output = std::process::Command::new(binary)
        .output()
        .unwrap_or_else(|e| panic!("could not run {binary}: {e}"));

    assert!(
        output.status.success(),
        "capability gate cases failed:\n{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// `confine` is deliberately not called in-process anywhere else: it cannot be
/// undone, so a test that installed a filter would constrain every test after
/// it in the same process.
#[test]
fn confine_is_documented_as_irreversible() {
    let _ = confine; // referenced so the symbol is exercised by the build
}
