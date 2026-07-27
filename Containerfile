# Linux packaging: one compile, three artifacts, then each one installed
# and actually started on a stock image of the distro family it targets.
#
# Why a container at all, rather than building on the CI runner: glibc is
# backward compatible but not forward compatible, so a binary picks up a
# floor from whatever glibc it was linked against and will not run on
# anything older. Building on `ubuntu-latest` means that floor silently
# rises every time GitHub updates the runner image, cutting off another
# tranche of users with no change on our side. Pinning the build to
# `ubuntu:22.04` fixes the floor at glibc 2.35 until we deliberately move
# it — which covers Debian 12+, Ubuntu 22.04+, and every current Fedora.
# (The deliberate exclusions at 2.35 are RHEL 9 at 2.34 and Debian 11 at
# 2.31; both can use the AppImage.)
#
# This file is the whole definition — CI runs exactly these stages, so a
# failure here reproduces locally with the same one command rather than
# only showing up in a workflow log after a tag has been pushed.

# ---------------------------------------------------------------------
# Build: compile once, then package the same binary three ways.
# ---------------------------------------------------------------------
FROM docker.io/library/ubuntu:22.04 AS builder

ENV DEBIAN_FRONTEND=noninteractive
ENV CARGO_TERM_COLOR=never
ENV PATH=/root/.cargo/bin:$PATH

# `dpkg-dev` is what supplies `dpkg-shlibdeps`, which cargo-deb's `$auto`
# uses to derive the libc floor — without it the .deb would ship with no
# glibc requirement at all and fail at runtime instead of refusing to
# install. `file` is a dependency of that same path.
#
# The X/Wayland/Vulkan `-dev` packages are deliberately *not* here. The
# release workflow installed them "very likely not actually required,
# unverified"; this image is that verification. Every one of those
# libraries is dlopen'd at runtime, so none is needed to link — if that
# were wrong, this stage would fail to build.
RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential \
        ca-certificates \
        curl \
        desktop-file-utils \
        dpkg-dev \
        file \
        pkg-config \
        zsync \
    && rm -rf /var/lib/apt/lists/*

ARG RUST_VERSION=stable
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- -y --profile minimal --default-toolchain "${RUST_VERSION}"

RUN cargo install cargo-deb --locked && cargo install cargo-generate-rpm --locked

# appimagetool is itself an AppImage, and AppImages need FUSE to mount
# themselves — which a container doesn't have. Extracting it once here and
# running the extracted tree directly sidesteps that entirely.
ARG APPIMAGETOOL_URL=https://github.com/AppImage/AppImageKit/releases/download/continuous/appimagetool-x86_64.AppImage
RUN curl -fsSL -o /tmp/appimagetool "${APPIMAGETOOL_URL}" \
    && chmod +x /tmp/appimagetool \
    && (cd /opt && /tmp/appimagetool --appimage-extract >/dev/null) \
    && mv /opt/squashfs-root /opt/appimagetool \
    && rm /tmp/appimagetool

WORKDIR /src
COPY . .

# No `--target`: the container is already the target, and letting cargo
# build natively keeps every packager's `target/release/...` asset path
# correct without threading a triple through all three of them.
RUN cargo build --release -p pain

# A binary that can't even report its own version is broken in a way worth
# catching before we wrap it in three packages. This proves nothing about
# dependencies — see the test stages for that.
RUN ./target/release/pain --version

# Each packager writes to its own default location and the results are
# copied out, rather than passing an output flag whose spelling has
# changed between versions of both tools.
RUN mkdir -p /out \
    && cargo deb -p pain --no-build \
    && cargo generate-rpm -p crates/app \
    && cp target/debian/*.deb /out/ \
    && cp target/generate-rpm/*.rpm /out/

# The plain tarball, for anyone who wants the binary without a package
# manager. Built here rather than on the runner so it inherits the same
# glibc floor as everything else.
RUN set -eux; \
    stage=/tmp/pain-linux-x86_64; \
    mkdir -p "$stage"; \
    cp target/release/pain LICENSE README.md "$stage/"; \
    tar -czf /out/pain-linux-x86_64.tar.gz -C /tmp pain-linux-x86_64

# A deliberately thin AppImage: the binary, a desktop entry, and an icon.
# Normally an AppImage bundles its libraries, but this one links nothing
# beyond libc/libm/libgcc — every windowing and GPU library is dlopen'd —
# and those specifically *must not* be bundled, because the Vulkan driver
# and Wayland/X11 client libraries have to match the host, not the build
# image. So bundling would buy nothing and break machines.
#
# `-u` embeds update information and emits the .zsync alongside, which is
# the only upgrade path an AppImage user gets. It requires the published
# URL to be stable, which is why the release job overwrites one fixed
# gh-pages path rather than publishing a versioned filename.
ARG APPIMAGE_UPDATE_URL=zsync|https://w-p.github.io/pain/appimage/pain-x86_64.AppImage.zsync
RUN set -eux; \
    appdir=/tmp/AppDir; \
    mkdir -p "$appdir/usr/bin" "$appdir/usr/share/applications" \
             "$appdir/usr/share/icons/hicolor/256x256/apps"; \
    cp target/release/pain "$appdir/usr/bin/pain"; \
    cp assets/pain.desktop "$appdir/usr/share/applications/pain.desktop"; \
    cp assets/pain-256.png "$appdir/usr/share/icons/hicolor/256x256/apps/pain.png"; \
    cp assets/pain.desktop "$appdir/pain.desktop"; \
    cp assets/pain-256.png "$appdir/pain.png"; \
    ln -s usr/bin/pain "$appdir/AppRun"; \
    desktop-file-validate "$appdir/pain.desktop"; \
    ARCH=x86_64 /opt/appimagetool/AppRun \
        -u "${APPIMAGE_UPDATE_URL}" \
        "$appdir" /out/pain-x86_64.AppImage; \
    # appimagetool writes the .zsync into the working directory rather than
    # beside the AppImage it was told to produce. Left unmoved it never gets
    # published, and the update URL embedded above points at nothing.
    mv pain-x86_64.AppImage.zsync /out/

# ---------------------------------------------------------------------
# Test stages: a clean image of each target family, nothing installed but
# the package and whatever its own dependencies pull in.
#
# `--version` alone would be a test that cannot fail: it returns before
# the event loop starts, so it never reaches the dlopen'd X11, Wayland,
# xkbcommon or Vulkan libraries — exactly the dependency list most likely
# to be wrong. These start the real application under a virtual display
# and expect it to survive: `timeout` reports 124 when it has to kill a
# process that was still running, so 124 is the pass and every other exit
# code means it died during startup.
# ---------------------------------------------------------------------
FROM docker.io/library/debian:12 AS test-deb
ENV DEBIAN_FRONTEND=noninteractive
COPY --from=builder /out/*.deb /tmp/
# `--no-install-recommends` on purpose: it proves the Vulkan driver really
# is pulled in as a hard dependency. With the driver left as a
# recommendation this install would succeed and the app would still fail
# to start, which is the bug this argument exists to catch.
#
# `xvfb`, `xauth` and a font are test scaffolding, not dependencies. A real
# machine gets the display from its graphical session and the fonts from
# its desktop, neither of which a bare base image has — the same reason no
# comparable terminal emulator requires a font package either.
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        /tmp/*.deb xvfb xauth fonts-dejavu-core \
    && rm -rf /var/lib/apt/lists/*
RUN pain --version
RUN xvfb-run -a timeout 5 pain --verbose; test $? -eq 124

FROM docker.io/library/fedora:latest AS test-rpm
COPY --from=builder /out/*.rpm /tmp/
RUN dnf install -y --setopt=install_weak_deps=False \
        /tmp/*.rpm xorg-x11-server-Xvfb xorg-x11-xauth dejavu-sans-mono-fonts \
    && dnf clean all
RUN pain --version
RUN xvfb-run -a timeout 5 pain --verbose; test $? -eq 124

# The AppImage is the fallback for distros we don't build a package for,
# so it's tested somewhere neither package was installed. `--appimage-
# extract-and-run` because containers have no FUSE; on a real machine
# without libfuse2 a user needs the same flag, which is why the README
# says so.
FROM docker.io/library/debian:12 AS test-appimage
ENV DEBIAN_FRONTEND=noninteractive
COPY --from=builder /out/pain-x86_64.AppImage /tmp/pain.AppImage
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        xvfb xauth libx11-6 libx11-xcb1 libxcursor1 libxi6 libxcb1 \
        libxkbcommon0 libxkbcommon-x11-0 libwayland-client0 libwayland-egl1 \
        libegl1 libvulkan1 mesa-vulkan-drivers fonts-dejavu-core \
    && rm -rf /var/lib/apt/lists/*
RUN chmod +x /tmp/pain.AppImage && /tmp/pain.AppImage --appimage-extract-and-run --version
RUN xvfb-run -a timeout 5 /tmp/pain.AppImage --appimage-extract-and-run --verbose; test $? -eq 124

# ---------------------------------------------------------------------
# Artifact-only stage, for copying the packages back out.
# ---------------------------------------------------------------------
FROM scratch AS artifacts
COPY --from=builder /out/ /
