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

# 网站代码在镜像构建时编译，发布文章无需重建镜像。
FROM node:24-bookworm-slim AS website-build
RUN npm install --global pnpm@11.9.0
ENV ASTRO_TELEMETRY_DISABLED=1
WORKDIR /site
COPY --from=frontend package.json pnpm-lock.yaml pnpm-workspace.yaml ./
COPY --from=frontend apps/website/package.json apps/website/
RUN --mount=type=cache,target=/pnpm/store \
    pnpm install --frozen-lockfile --filter @silva/website... --store-dir /pnpm/store
COPY --from=frontend apps/website apps/website
RUN pnpm --filter @silva/website build:ssr

FROM node:24-bookworm-slim AS website
WORKDIR /site/apps/website
COPY --from=website-build /site/node_modules /site/node_modules
COPY --from=website-build /site/apps/website/node_modules ./node_modules
COPY --from=website-build /site/apps/website/dist ./dist
COPY --from=website-build /site/apps/website/package.json ./
USER node
ENV HOST=0.0.0.0 PORT=4321 NODE_ENV=production
EXPOSE 4321
CMD ["node", "dist/server/entry.mjs"]

FROM runtime AS backend
