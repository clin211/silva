# syntax=docker/dockerfile:1
FROM rust:1.100.0-bookworm AS builder
WORKDIR /build
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/build/target \
    cargo build --release --locked --workspace \
    && mkdir /out \
    && cp target/release/silva-api target/release/silva-worker target/release/silva-cli /out/ \
    && mkdir /out-tools \
    && cp target/release/silva-xtask /out-tools/

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 10001 silva \
    && useradd --uid 10001 --gid silva --no-create-home --shell /usr/sbin/nologin silva
COPY --from=builder /out/ /usr/local/bin/
USER silva:silva
WORKDIR /app
ENV SILVA_BIND=0.0.0.0:8088
EXPOSE 8088
STOPSIGNAL SIGTERM
CMD ["silva-api"]

FROM runtime AS observability-tools
COPY --from=builder /out-tools/ /usr/local/bin/
CMD ["silva-xtask", "alert-receiver"]

FROM runtime AS backend
