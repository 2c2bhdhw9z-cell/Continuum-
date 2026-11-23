#!/bin/sh
# Sets up everything needed to build Continuum for the LG TV in a fresh sandbox, in $TVB
# (default /projects/sandbox/.tvbuild; /tmp does not survive between commands there):
#   - Rust's armv7-unknown-linux-gnueabi standard library for the pinned 1.92.0 (the TV's
#     programs are 32-bit ARM, SOFT-float ABI, /lib/ld-linux.so.3)
#   - a glibc 2.31 sysroot from Debian 11 armel (archive.debian.org; deb.debian.org dropped armel)
#   - arm-cc (clang + lld) and env.sh to source before a build
# Afterwards, copy SDL2 from the TV once (needs the SSH tunnel; see TV_HANDOFF.md):
#   tv 'cat /media/developer/apps/usr/palm/applications/dev.continuum.tv/lib/libSDL2-2.0.so.0' > $TVB/tvlibs/libSDL2-2.0.so.0
set -eu
TVB=${TVB:-/projects/sandbox/.tvbuild}
mkdir -p "$TVB/rustsys/lib/rustlib" "$TVB/sysroot" "$TVB/tvlibs" "$TVB/cargo-home"
cd "$TVB"
if [ ! -d rustsys/lib/rustlib/armv7-unknown-linux-gnueabi ]; then
  curl -sfL https://static.rust-lang.org/dist/rust-std-1.92.0-armv7-unknown-linux-gnueabi.tar.xz | tar -xJ
  mv rust-std-1.92.0-armv7-unknown-linux-gnueabi/rust-std-armv7-unknown-linux-gnueabi/lib/rustlib/armv7-unknown-linux-gnueabi rustsys/lib/rustlib/
  rm -rf rust-std-1.92.0-armv7-unknown-linux-gnueabi
fi
H=$(RUSTUP_TOOLCHAIN=1.92.0-x86_64-unknown-linux-gnu rustc --print sysroot)
ln -sfn "$H/lib/rustlib/x86_64-unknown-linux-gnu" rustsys/lib/rustlib/x86_64-unknown-linux-gnu
if [ ! -e sysroot/lib/ld-linux.so.3 ]; then
  A=https://archive.debian.org/debian
  curl -sfL $A/dists/bullseye/main/binary-armel/Packages.xz | xz -dc > P
  for p in libc6 libc6-dev linux-libc-dev libgcc-10-dev libgcc-s1; do
    awk -v p="$p" '$0=="Package: "p{f=1} f&&/^Filename:/{print $2;f=0}' P
  done > files
  rm -f P; mkdir -p debs
  while read f; do (cd debs && curl -sfLO "$A/$f"); done < files
  for d in debs/*.deb; do mkdir -p x && (cd x && ar x "../$d" && tar -xf data.tar.* -C ../sysroot && rm -f ./*); done
  rm -rf debs x files
  # absolute links inside the packages must point inside the sysroot, or lld picks static libs
  find sysroot -type l | while read l; do t=$(readlink "$l"); case "$t" in /*) ln -sfn "$TVB/sysroot$t" "$l";; esac; done
fi
cat > arm-cc <<EOT
#!/bin/sh
exec clang --target=armv7-unknown-linux-gnueabi -mfloat-abi=softfp -mfpu=neon --sysroot=$TVB/sysroot -fuse-ld=lld -Wno-unused-command-line-argument "\$@"
EOT
chmod +x arm-cc
cat > env.sh <<EOT
# source this before a TV build or a cboy-bench run
T=$TVB
export TV_BUILD=$TVB
export RUSTUP_TOOLCHAIN=1.92.0-x86_64-unknown-linux-gnu
export CARGO_HOME=$TVB/cargo-home
export TV_TARGET=armv7-unknown-linux-gnueabi
export CARGO_TARGET_ARMV7_UNKNOWN_LINUX_GNUEABI_LINKER=$TVB/arm-cc
export CARGO_TARGET_ARMV7_UNKNOWN_LINUX_GNUEABI_RUSTFLAGS="--sysroot $TVB/rustsys -C target-cpu=cortex-a55 -C target-feature=-soft-float,+neon,+fp-armv8"
export CC_armv7_unknown_linux_gnueabi=$TVB/arm-cc
export AR_armv7_unknown_linux_gnueabi=llvm-ar
EOT
echo "ready: . $TVB/env.sh"
