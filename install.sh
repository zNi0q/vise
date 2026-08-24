#!/bin/sh
# Install the Vise toolchain.
#
#   ./install.sh                 install to ~/.local/bin
#   PREFIX=/usr/local ./install.sh   install to /usr/local/bin
#   ./install.sh --uninstall     remove it again
#
# POSIX sh on purpose: this has to run before anything else is installed.

set -eu

PREFIX="${PREFIX:-$HOME/.local}"
BINDIR="$PREFIX/bin"
NAME=vise

say() { printf '%s\n' "$*"; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }

if [ "${1:-}" = "--uninstall" ]; then
    if [ -e "$BINDIR/$NAME" ]; then
        rm -f "$BINDIR/$NAME"
        say "removed $BINDIR/$NAME"
    else
        say "$BINDIR/$NAME is not installed"
    fi
    exit 0
fi

if [ "${1:-}" = "--help" ] || [ "${1:-}" = "-h" ]; then
    sed -n '2,10p' "$0" | sed 's/^# \{0,1\}//'
    exit 0
fi

command -v cargo >/dev/null 2>&1 || die "cargo is required; install Rust from https://rustup.rs"
command -v cc >/dev/null 2>&1 || die "a C compiler is required: the runtime and the backend both need one"

# The build script compiles the C runtime, so a missing compiler fails here
# rather than at the first `vise build`.
say "building (this takes a minute the first time)"
cargo build --release --locked --quiet || die "the build failed"

BUILT="target/release/$NAME"
[ -x "$BUILT" ] || die "expected $BUILT to exist after a successful build"

mkdir -p "$BINDIR"
install -m 755 "$BUILT" "$BINDIR/$NAME"
say "installed $BINDIR/$NAME"

# Report the version through the binary that was just installed, so a stale one
# earlier in PATH shows up now rather than confusing someone later.
FOUND=$(command -v "$NAME" 2>/dev/null || true)
if [ -z "$FOUND" ]; then
    say ""
    say "$BINDIR is not on your PATH. Add this to your shell profile:"
    say "    export PATH=\"\$PATH:$BINDIR\""
elif [ "$FOUND" != "$BINDIR/$NAME" ]; then
    say ""
    say "warning: '$NAME' on your PATH is $FOUND, not the one just installed."
else
    say "$("$BINDIR/$NAME" version)"
    say ""
    say "try:  $NAME repl"
fi
