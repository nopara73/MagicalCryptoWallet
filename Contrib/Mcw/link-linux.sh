#!/bin/sh
# The aborting, backtrace-free standard library does not use GCC's unwinder.
# GNU ARM ld retains its empty DT_NEEDED entry despite --as-needed; omit the
# request so a future actual unwinder dependency fails at link time.
set -eu
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
