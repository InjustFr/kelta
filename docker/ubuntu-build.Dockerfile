FROM ubuntu:24.04

# Published as ghcr.io/injustfr/kelta-ci by .github/workflows/ci-image.yml; the label links the package to the repo.
LABEL org.opencontainers.image.source=https://github.com/InjustFr/kelta

ENV DEBIAN_FRONTEND=noninteractive \
    RUSTUP_HOME=/root/.rustup \
    CARGO_HOME=/root/.cargo \
    PATH=/root/.cargo/bin:$PATH \
    COREPACK_ENABLE_DOWNLOAD_PROMPT=0

RUN apt-get update && apt-get install -y --no-install-recommends \
      build-essential pkg-config curl ca-certificates git rsync file python3 lsof zstd \
      desktop-file-utils appstream \
      libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev libayatana-appindicator3-dev \
      libssl-dev libxdo-dev patchelf \
    && rm -rf /var/lib/apt/lists/*

# The Neovim CI installs (scripts/install-nvim.sh), not apt's older one.
COPY scripts/install-nvim.sh /tmp/install-nvim.sh
RUN bash /tmp/install-nvim.sh

RUN curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain 1.99.0 -c rustfmt -c clippy

# cargo-nextest for scripts/qa.sh (prebuilt release, pinned).
RUN curl -fsSL "https://get.nexte.st/0.9.148/$([ "$(uname -m)" = aarch64 ] && echo linux-arm || echo linux)" \
      | tar -xz -C /root/.cargo/bin

RUN curl -fsSL https://deb.nodesource.com/setup_22.x | bash - \
    && apt-get install -y --no-install-recommends nodejs \
    && rm -rf /var/lib/apt/lists/* \
    && corepack enable \
    && corepack prepare pnpm@9 --activate

RUN mkdir -p /work
WORKDIR /work
