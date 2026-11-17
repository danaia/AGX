#!/bin/bash
# Run as root on the Pi after verified artifacts are staged on NVMe.
set -euo pipefail
base=/var/lib/agx-data/angi-sdk
mountpoint -q /var/lib/agx-data
mkdir -p "$base"/{tools,sysroot,bin}
tar -xf "$base/sdk.tar" -C "$base/tools"
tar -xf "$base/sysroot.tar" -C "$base/sysroot"
for name in as ld; do ln -sfn "aarch64-linux-gnu-$name" "$base/tools/usr/bin/$name"; done
cat > "$base/bin/tool" <<'EOF'
#!/bin/bash
set -euo pipefail
sdk=/var/lib/agx-data/angi-sdk
export LD_LIBRARY_PATH=$sdk/tools/usr/lib/aarch64-linux-gnu:$sdk/tools/lib/aarch64-linux-gnu
case "${0##*/}" in
 python3)
    export PYTHONHOME=$sdk/tools/usr
    exec "$sdk/tools/usr/bin/python3.11" "$@" ;;
 cc|gcc|c++|g++)
    export GCC_EXEC_PREFIX=$sdk/tools/usr/lib/gcc/
    export COMPILER_PATH=$sdk/tools/usr/bin
    export PATH=$sdk/tools/usr/bin:$PATH
    compiler=aarch64-linux-gnu-gcc-12
    case "${0##*/}" in c++|g++) compiler=aarch64-linux-gnu-g++-12;; esac
    exec "$sdk/tools/usr/bin/$compiler" --sysroot="$sdk/sysroot" -B"$sdk/tools/usr/lib/gcc/aarch64-linux-gnu/12/" "$@" ;;
 pkg-config) exec "$sdk/tools/usr/bin/pkgconf" "$@" ;;
 make) exec "$sdk/tools/usr/bin/make" "$@" ;;
 *) echo 'Unknown native SDK tool' >&2; exit 1 ;;
esac
EOF
chmod 755 "$base/bin/tool"
for name in python3 cc gcc c++ g++ pkg-config make; do
    ln -sfn tool "$base/bin/$name"
    ln -sfn "$base/bin/$name" "/usr/bin/$name"
done
"$base/bin/cc" --version | head -1
"$base/bin/python3" -c 'import curses, sqlite3, ssl; print("Native Python, curses, SQLite and TLS ready")'
export RUSTUP_HOME=$base/rustup CARGO_HOME=$base/cargo
if ! test -x "$base/cargo/bin/rustc"; then
    chmod 755 "$base/rustup-init"
    "$base/rustup-init" -y --no-modify-path --profile minimal --default-toolchain 1.97.1
fi
"$base/cargo/bin/rustup" component add --toolchain 1.97.1 rustfmt clippy
chmod -R a+rX "$base/rustup" "$base/cargo"
"$base/cargo/bin/rustc" --version
