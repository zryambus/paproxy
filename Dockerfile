FROM rust:1.97.1-alpine3.24 AS rust

RUN apk add musl-dev
WORKDIR /home/rust/src

# Build dependencies in a separate layer, cached until Cargo.toml or Cargo.lock change
COPY Cargo.toml Cargo.lock LICENSE ./
RUN mkdir src && echo "fn main() {}" > src/main.rs &&     cargo build --release --locked &&     rm -rf src

COPY src src
# Make sure the real sources are newer than the dummy build
RUN touch src/main.rs && cargo build --release --locked

FROM scratch
COPY --from=rust /home/rust/src/target/release/paproxy /paproxy
ENTRYPOINT [ "/paproxy" ]
