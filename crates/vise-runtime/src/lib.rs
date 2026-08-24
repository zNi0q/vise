//! The Vise runtime: the half of the effect system the kernel enforces.
//!
//! Spec §7 requires that "static checking and runtime confinement agree, or the
//! program does not run". [`confine`] is the confinement half: it narrows the
//! process to the syscalls an effect row implies, so a mistake in the effect
//! checker cannot become a hole in the sandbox.
//!
//! See `runtime/c/capability.h` for what syscall filtering can and cannot
//! reach. The short version: `fs`, `net`, `rand`, `proc`, and the descriptor
//! side of `io` are enforced twice; `time` and `env` are enforced by the
//! compiler alone, because neither reaches the kernel.

use std::ffi::{CStr, c_char};
use std::fmt;

use vise_ast::Effect;

#[cfg(not(vise_no_runtime))]
unsafe extern "C" {
    fn vise_caps_apply(effects: u32) -> u32;
    fn vise_caps_syscall_count(effects: u32) -> u32;
    fn vise_caps_strerror(result: u32) -> *const c_char;
}

/// Why confinement could not be installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfineError {
    code: u32,
    message: String,
}

impl ConfineError {
    /// The raw result code from the C layer.
    #[must_use]
    pub const fn code(&self) -> u32 {
        self.code
    }
}

impl fmt::Display for ConfineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ConfineError {}

/// The bit for one effect.
///
/// The order matches `Effect`'s declaration and the constants in
/// `capability.h`. A test asserts all three agree, because a silent drift here
/// would confine a program to the wrong set.
#[must_use]
pub const fn bit(effect: Effect) -> u32 {
    match effect {
        Effect::Io => 1 << 0,
        Effect::Fs => 1 << 1,
        Effect::Net => 1 << 2,
        Effect::Time => 1 << 3,
        Effect::Rand => 1 << 4,
        Effect::Env => 1 << 5,
        Effect::Proc => 1 << 6,
    }
}

/// The bit set for a whole effect row.
#[must_use]
pub fn bits(effects: &[Effect]) -> u32 {
    effects.iter().copied().fold(0, |acc, e| acc | bit(e))
}

/// How many syscalls a row permits.
///
/// Exposed mainly so a test can confirm that widening a row only ever adds
/// syscalls, without installing a filter it could not then undo.
#[must_use]
pub fn syscall_count(effects: &[Effect]) -> u32 {
    #[cfg(vise_no_runtime)]
    {
        let _ = effects;
        0
    }
    #[cfg(not(vise_no_runtime))]
    // SAFETY: reads a fixed table in C and returns a count. No pointers cross
    // the boundary and the call has no side effects.
    unsafe {
        vise_caps_syscall_count(bits(effects))
    }
}

/// Confine this process to the syscalls `effects` implies.
///
/// **Irreversible.** A seccomp filter can be narrowed but never removed, so
/// this is a one-way door for the whole process. Call it immediately before
/// running user code, never from a library.
///
/// A denied syscall kills the process with `SIGSYS` rather than returning an
/// error: a program that has escaped its declared effects does not get to
/// decide what to do about that.
///
/// # Errors
/// Returns [`ConfineError`] when the kernel lacks seccomp support, rejects the
/// filter, or the platform has no gate.
pub fn confine(effects: &[Effect]) -> Result<(), ConfineError> {
    #[cfg(vise_no_runtime)]
    {
        let _ = effects;
        Err(ConfineError {
            code: 5,
            message: "no capability gate for this platform".to_owned(),
        })
    }
    #[cfg(not(vise_no_runtime))]
    {
        // SAFETY: passes an integer to C. The call installs a kernel filter and
        // returns a status; nothing is allocated or borrowed across the
        // boundary.
        let code = unsafe { vise_caps_apply(bits(effects)) };
        if code == 0 {
            return Ok(());
        }
        // SAFETY: the C function returns a pointer to a static string literal
        // for every input, including unknown codes.
        let message = unsafe { CStr::from_ptr(vise_caps_strerror(code)) }
            .to_string_lossy()
            .into_owned();
        Err(ConfineError { code, message })
    }
}
