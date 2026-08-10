FROM rust:1.97.1-alpine3.24 AS rust

COPY . /home/rust/src
RUN apk add musl-dev && cd /home/rust/src && \
    cargo build --release --target-dir target/x86_64-unknown-linux-musl

FROM scratch
COPY --from=rust /home/rust/src/target/x86_64-unknown-linux-musl/release/paproxy /paproxy
ENTRYPOINT [ "/paproxy" ]
