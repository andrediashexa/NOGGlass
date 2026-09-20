# NOGGlass, built and shipped as one binary (ADR-0012).
#
# Two stages: everything needed to compile stays in the builder, and the
# runtime image carries the binary and nothing else — no compiler, no package
# manager, no source. An attacker who finds a way to run code in there finds no
# tools to build anything with.
#
# The builder always runs on the **native** architecture and cross-compiles,
# rather than being emulated for the target. Emulating a Rust compile of ring,
# rustls and russh under QEMU ran past 30 minutes on the first release before
# it was cancelled; cross-compiling takes about as long as a native build.
# zig provides the cross-linker, which is what makes that cheap: one small
# download instead of a toolchain per target.

FROM --platform=$BUILDPLATFORM rust:1.90-alpine AS builder

RUN apk add --no-cache musl-dev pkgconfig curl xz tar

# zig, as the cross-linker. Pinned: a linker that changes under us changes the
# binary without anything in this repository changing.
ARG ZIG_VERSION=0.16.0
RUN set -eux; \
    arch="$(uname -m)"; \
    curl -fsSL "https://ziglang.org/download/${ZIG_VERSION}/zig-${arch}-linux-${ZIG_VERSION}.tar.xz" -o /tmp/zig.tar.xz; \
    mkdir -p /opt/zig; \
    tar -xJf /tmp/zig.tar.xz -C /opt/zig --strip-components=1; \
    rm /tmp/zig.tar.xz; \
    ln -s /opt/zig/zig /usr/local/bin/zig; \
    zig version

RUN cargo install cargo-zigbuild --locked --version 0.20.1

# BUILDPLATFORM is where we are; TARGETARCH is what we are building for.
ARG TARGETARCH
RUN set -eux; \
    case "$TARGETARCH" in \
      amd64) target=x86_64-unknown-linux-musl ;; \
      arm64) target=aarch64-unknown-linux-musl ;; \
      *) echo "unsupported target architecture: $TARGETARCH" >&2; exit 1 ;; \
    esac; \
    echo "$target" > /rust-target; \
    rustup target add "$target"

WORKDIR /build

# Dependencies first, so editing source does not rebuild the dependency tree.
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates/looking-glass-core/Cargo.toml crates/looking-glass-core/
COPY crates/nogglass-server/Cargo.toml crates/nogglass-server/
RUN set -eux; \
    mkdir -p crates/looking-glass-core/src crates/nogglass-server/src; \
    echo 'fn main() {}' > crates/nogglass-server/src/main.rs; \
    touch crates/looking-glass-core/src/lib.rs; \
    cargo zigbuild --release --workspace --target "$(cat /rust-target)"; \
    rm -rf crates/looking-glass-core/src crates/nogglass-server/src

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
RUN set -eux; \
    touch crates/*/src/*.rs; \
    target="$(cat /rust-target)"; \
    cargo zigbuild --release --workspace --target "$target"; \
    strip "target/$target/release/nogglass"; \
    cp "target/$target/release/nogglass" /nogglass

FROM alpine:3.21

# ca-certificates so the optional RPKI and global-view lookups can verify TLS;
# tini so signals reach the process and a restart does not wait on a stuck
# container.
RUN apk add --no-cache ca-certificates tini \
    && addgroup -g 10001 -S nogglass \
    && adduser -u 10001 -S -G nogglass -h /nonexistent -s /sbin/nologin nogglass \
    && mkdir -p /etc/nogglass \
    && chown nogglass:nogglass /etc/nogglass

COPY --from=builder /nogglass /usr/local/bin/nogglass

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
