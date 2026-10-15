# DEC-11 spike (throwaway): Linux → Windows cross build with Fedora's MinGW GTK 4, and Wine
# with Xvfb to run what it builds.
#   docker build -t cronkite-xwin -f spikes/dec-11/windows/cross.Dockerfile spikes/dec-11/windows
FROM fedora:latest
RUN dnf -y install --setopt=install_weak_deps=False \
        mingw64-gtk4 mingw64-gcc mingw64-pkg-config mingw64-adwaita-icon-theme \
        gcc curl findutils xorg-x11-server-Xvfb ImageMagick xdotool wine \
        mingw64-gdk-pixbuf zip \
    && dnf clean all
ENV RUSTUP_HOME=/opt/rustup CARGO_HOME=/opt/cargo PATH=/opt/cargo/bin:$PATH
RUN curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable \
        --target x86_64-pc-windows-gnu \
    && rustup component add rustfmt clippy
ENV CARGO_BUILD_TARGET=x86_64-pc-windows-gnu \
    CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc \
    PKG_CONFIG_ALLOW_CROSS=1 \
    PKG_CONFIG=x86_64-w64-mingw32-pkg-config \
    CARGO_TARGET_DIR=/build/target
