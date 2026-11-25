# =============================================================================
# 多阶段构建：Rust 后端 + Vue 前端 → 单个运行镜像
#
# 交付形态：
#   docker build -t zcadmin:0.1.0 .
#   docker run -p 8080:8080 -e DATABASE_URL=... zcadmin:0.1.0
#
# 镜像只包含：二进制 + 前端静态资源 + 配置，不含编译工具链。
# =============================================================================

# ---------- 1. 编译后端 ----------
FROM rust:1-bookworm AS backend
WORKDIR /app

# 先拷依赖清单，命中缓存时无需重新下载依赖（源码变化不影响这一层）
COPY Cargo.toml Cargo.lock ./
COPY crates/kernel/Cargo.toml crates/kernel/Cargo.toml
COPY crates/xtask/Cargo.toml crates/xtask/Cargo.toml
COPY plugins/demo/Cargo.toml plugins/demo/Cargo.toml
COPY plugins/crm/Cargo.toml plugins/crm/Cargo.toml
COPY plugins/workflow/Cargo.toml plugins/workflow/Cargo.toml
COPY apps/server/Cargo.toml apps/server/Cargo.toml

# 拷贝源码并编译（xtask 是工作区成员，目录必须存在）
COPY crates ./crates
COPY plugins ./plugins
COPY apps ./apps
RUN cargo build --release -p server

# ---------- 2. 编译前端 ----------
FROM node:22-alpine AS frontend
WORKDIR /web
COPY frontend/package.json frontend/package-lock.json ./
RUN npm ci --no-audit --no-fund
COPY frontend/ ./
RUN npm run build

# ---------- 3. 运行镜像 ----------
FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --create-home app

WORKDIR /app
COPY --from=backend /app/target/release/server /app/server
COPY --from=frontend /web/dist /app/web
COPY config /app/config

# 文件存储目录（生产建议挂载卷）
RUN mkdir -p /app/data && chown -R app:app /app

USER app
ENV CONFIG_PATH=/app/config/default.toml \
    WEB_DIST=/app/web \
    BIND_ADDR=0.0.0.0:8080 \
    RUST_LOG=info \
    LOG_FORMAT=json

# 让 PID 1 能正确转发 SIGTERM 并回收僵尸进程（tini 在 debian-slim 需单独装，这里用 exec 形式）
STOPSIGNAL SIGTERM

EXPOSE 8080

# 健康检查：容器内没有 curl，用二进制自带的 --healthcheck（TCP 探活）
HEALTHCHECK --interval=30s --timeout=3s --start-period=20s --retries=3 \
    CMD ["/app/server", "--healthcheck"]

ENTRYPOINT ["/app/server"]
