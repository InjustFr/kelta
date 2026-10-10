FROM ubuntu:24.04

ENV DEBIAN_FRONTEND=noninteractive \
    RUSTUP_HOME=/root/.rustup \
    CARGO_HOME=/root/.cargo \
    PATH=/root/.cargo/bin:$PATH \
    COREPACK_ENABLE_DOWNLOAD_PROMPT=0

RUN apt-get update && apt-get install -y --no-install-recommends \
      build-essential pkg-config curl ca-certificates git rsync file neovim python3 \
      libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev libayatana-appindicator3-dev \
      libssl-dev libxdo-dev patchelf \
    && rm -rf /var/lib/apt/lists/*

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
