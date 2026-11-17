# Source this file for native builds on ANGI. Never add these libraries to /lib.
export AGX_NATIVE_SDK=/var/lib/agx-data/angi-sdk
export RUSTUP_HOME=$AGX_NATIVE_SDK/rustup
export RUSTUP_TOOLCHAIN=1.97.1
export CARGO_HOME=/var/lib/agx-data/agx-opencode-users/$(id -u)/cargo
export CARGO_TARGET_DIR=$CARGO_HOME/target
export PATH=$AGX_NATIVE_SDK/bin:$AGX_NATIVE_SDK/cargo/bin:$PATH
export CC=$AGX_NATIVE_SDK/bin/cc CXX=$AGX_NATIVE_SDK/bin/c++
export AR=$AGX_NATIVE_SDK/tools/usr/bin/aarch64-linux-gnu-ar
export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=$CC
export PKG_CONFIG=$AGX_NATIVE_SDK/bin/pkg-config
export PKG_CONFIG_SYSROOT_DIR=$AGX_NATIVE_SDK/sysroot
export PKG_CONFIG_LIBDIR=$AGX_NATIVE_SDK/sysroot/usr/lib/pkgconfig:$AGX_NATIVE_SDK/sysroot/usr/share/pkgconfig
