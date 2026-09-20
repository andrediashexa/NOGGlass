# NOGGlass, built and shipped as one binary (ADR-0012).
#
# Two stages: everything needed to compile stays in the builder, and the
# runtime image carries the binary and nothing else — no compiler, no package
# manager, no source. An attacker who finds a way to run code in there finds no
# tools to build anything with.

FROM rust:1.90-alpine AS builder

# musl-dev and the assembler are needed by ring; pkgconfig by the TLS stack.
RUN apk add --no-cache musl-dev pkgconfig

WORKDIR /build

# Dependencies first, so editing source does not rebuild the dependency tree.
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates/looking-glass-core/Cargo.toml crates/looking-glass-core/
COPY crates/nogglass-server/Cargo.toml crates/nogglass-server/
RUN mkdir -p crates/looking-glass-core/src crates/nogglass-server/src \
    && echo 'fn main() {}' > crates/nogglass-server/src/main.rs \
    && touch crates/looking-glass-core/src/lib.rs \
    && cargo build --release --workspace \
    && rm -rf crates/*/src

COPY crates ./crates

# The release build stamps the version, so /api/version and the footer report
# what is actually running instead of "dev".
ARG NOGGLASS_VERSION=dev
ARG NOGGLASS_COMMIT=unknown
ARG NOGGLASS_BUILT_AT=unknown
ENV NOGGLASS_VERSION=$NOGGLASS_VERSION \
    NOGGLASS_COMMIT=$NOGGLASS_COMMIT \
    NOGGLASS_BUILT_AT=$NOGGLASS_BUILT_AT

# Touch the sources so the dependency-cache trick above does not leave a stale
# binary behind.
RUN touch crates/*/src/*.rs && cargo build --release --workspace \
    && strip target/release/nogglass

FROM alpine:3.21

# ca-certificates so the optional RPKI lookup can verify TLS; tini so signals
# reach the process and a restart does not wait on a stuck container.
RUN apk add --no-cache ca-certificates tini \
    && addgroup -g 10001 -S nogglass \
    && adduser -u 10001 -S -G nogglass -h /nonexistent -s /sbin/nologin nogglass \
    && mkdir -p /etc/nogglass \
    && chown nogglass:nogglass /etc/nogglass

COPY --from=builder /build/target/release/nogglass /usr/local/bin/nogglass

USER nogglass:nogglass

# Unprivileged port: binding 80 or 443 needs a capability or a proxy, and the
# deployment guide covers both (ADR-0012).
EXPOSE 8080
ENV NOGGLASS_CONFIG=/etc/nogglass/nogglass.toml \
    NOGGLASS_HTTP_ADDR=0.0.0.0:8080

# The health endpoint answers without touching a router, so a health check
# never costs control-plane CPU.
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD wget -qO- http://127.0.0.1:8080/api/health || exit 1

ENTRYPOINT ["/sbin/tini", "--", "/usr/local/bin/nogglass"]
