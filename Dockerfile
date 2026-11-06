# Terminal build: static musl binary, also used for the runtime image
FROM rust:1.97.1-alpine3.24 AS rust

RUN apk add musl-dev
WORKDIR /home/rust/src

# Build dependencies in a separate layer, cached until Cargo.toml or Cargo.lock change
COPY Cargo.toml Cargo.lock LICENSE build.rs ./
RUN mkdir src && echo "fn main() {}" > src/main.rs && \
    cargo build --release --locked && \
    rm -rf src

COPY src src
# Make sure the real sources are newer than the dummy build
RUN touch src/main.rs && cargo build --release --locked

# GUI build: GPUI needs glibc and the X11/Wayland libraries
FROM rust:1.97.1-bookworm AS gui

RUN apt-get update && apt-get install -y --no-install-recommends \
    cmake clang pkg-config \
    libfontconfig-dev libfreetype-dev \
    libwayland-dev libxkbcommon-dev libxkbcommon-x11-dev libx11-xcb-dev libxcb1-dev \
    libvulkan-dev \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /home/rust/src

COPY Cargo.toml Cargo.lock LICENSE build.rs ./
RUN mkdir src && echo "fn main() {}" > src/main.rs && \
    cargo build --release --locked --features gui && \
    rm -rf src

COPY src src
# The window icon is embedded from assets
COPY assets assets
RUN touch src/main.rs && cargo build --release --locked --features gui

# Release binaries, exported with: docker build --target release --output=build .
FROM scratch AS release
COPY --from=rust /home/rust/src/target/release/paproxy /paproxy
COPY --from=gui /home/rust/src/target/release/paproxy /paproxy-gui

# Runtime image, the default target
FROM scratch
COPY --from=rust /home/rust/src/target/release/paproxy /paproxy
ENTRYPOINT [ "/paproxy" ]
