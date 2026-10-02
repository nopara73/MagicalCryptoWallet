#!/bin/sh
# The aborting, backtrace-free standard library does not use GCC's unwinder.
# GNU ARM ld retains its empty DT_NEEDED entry despite --as-needed; omit the
# request so a future actual unwinder dependency fails at link time.
set -eu
# Compiler build scripts use the prebuilt unwinding standard library. They are
# build tools, so preserve their linker inputs and change only the shipping bin.
if [ "${CARGO_BIN_NAME:-}" != mcw ]; then
    exec "${MCW_NATIVE_LINKER:-cc}" -Wl,--as-needed "$@"
fi
remaining=$#
while [ "$remaining" -gt 0 ]; do
    argument=$1
    shift
    remaining=$((remaining - 1))
    case "$argument" in
        -lgcc_s) ;;
        *) set -- "$@" "$argument" ;;
    esac
done
exec "${MCW_NATIVE_LINKER:-cc}" -Wl,--as-needed "$@"
