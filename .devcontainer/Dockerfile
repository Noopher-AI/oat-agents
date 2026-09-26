# The environment this repository's commands run in.
#
# Pinned to the toolchain the crate will declare: edition 2024 needs Rust 1.85
# or newer. tmux is here because the core launches every agent in a tmux
# session and its tests exercise that.
FROM docker.io/library/rust:1.85-slim-bookworm

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        git \
        pkg-config \
        tmux \
    && rm -rf /var/lib/apt/lists/*

RUN rustup component add clippy rustfmt
