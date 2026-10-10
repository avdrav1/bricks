# Release build environment (PKG-3). The binary links against this image's glibc and
# GTK 4, so building on Ubuntu 24.04 (glibc 2.39, GTK 4.14: the oldest we support) makes a
# binary that also starts on newer systems: Arch, Fedora. `scripts/ship.sh build` runs
# cargo in it as the calling user, with the repo mounted at /src.
FROM ubuntu:24.04

RUN apt-get update \
 && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
      build-essential ca-certificates curl pkg-config libgtk-4-dev \
 && rm -rf /var/lib/apt/lists/*

# Toolchain readable by any user; downloads go to CARGO_HOME, set per build. The
# components rust-toolchain.toml names are installed here: rustup can't add them later,
# as the build runs as the calling user.
ENV RUSTUP_HOME=/opt/rustup PATH=/opt/cargo/bin:$PATH
RUN curl -fsSL https://sh.rustup.rs | CARGO_HOME=/opt/cargo sh -s -- -y --profile minimal \
      --default-toolchain stable --component rustfmt,clippy --no-modify-path \
 && chmod -R a+rX /opt/rustup /opt/cargo
