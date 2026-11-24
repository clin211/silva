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

# 网站依赖：Worker 构建公开站点时在此目录执行 Astro。前端目录以命名上下文 `frontend` 传入：
# docker build --build-context frontend=../frontend --target worker .
FROM node:24-bookworm-slim AS website
RUN npm install --global pnpm@11.9.0
WORKDIR /site
COPY --from=frontend package.json pnpm-lock.yaml pnpm-workspace.yaml ./
COPY --from=frontend apps/website/package.json apps/website/
RUN --mount=type=cache,target=/pnpm/store \
    pnpm install --frozen-lockfile --filter @silva/website... --store-dir /pnpm/store
COPY --from=frontend apps/website apps/website

# 带站点构建能力的 Worker：包含 Node、pnpm 与已安装依赖的网站工程。
# pnpm 在镜像构建时安装，运行时不访问 npm registry。
FROM node:24-bookworm-slim AS worker
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && npm install --global pnpm@11.9.0 \
    && groupadd --gid 10001 silva \
    && useradd --uid 10001 --gid silva --no-create-home --shell /usr/sbin/nologin silva
COPY --from=builder /out/silva-worker /usr/local/bin/
COPY --from=website --chown=silva:silva /site /site
USER silva:silva
WORKDIR /app
# 运行用户没有家目录；构建不发送遥测。
ENV SILVA_SITE_BUILDER_DIR=/site/apps/website \
    HOME=/tmp \
    ASTRO_TELEMETRY_DISABLED=1
STOPSIGNAL SIGTERM
CMD ["silva-worker"]

# 默认目标：API、Worker（不含站点构建）与 CLI。
FROM runtime AS backend
