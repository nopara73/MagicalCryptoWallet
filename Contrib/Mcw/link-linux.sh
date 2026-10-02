#!/bin/sh
# Apply --as-needed before Rust's native library arguments. A trailing
# -C link-arg cannot remove an earlier unused libgcc_s on the GNU ARM linker.
set -eu
exec "${MCW_NATIVE_LINKER:-cc}" -Wl,--as-needed "$@"
