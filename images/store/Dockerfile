# 镜像：两段构建，运行镜像里只有二进制和 SQLite 需要的系统库。
# 许可证：rust 镜像是 MIT OR Apache-2.0，alpine 是各包自己的许可证（独立容器，不链接进二进制）。

# 基础镜像默认走 Docker Hub；本机到 Docker Hub 不通时可以换镜像站，用 --build-arg 覆盖两个 ARG。
ARG RUST_IMAGE=rust:1.85-alpine
ARG RUNTIME_IMAGE=alpine:3.22

FROM ${RUST_IMAGE} AS build
# rusqlite 用 bundled 编译 SQLite 的 C 源码，构建镜像里要有 C 工具链。
RUN apk add --no-cache build-base
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
# 依赖先单独编一层，改 src 不用重编依赖。
RUN mkdir src && echo 'fn main() {}' > src/main.rs && cargo build --release --locked && rm -rf src
COPY src ./src
RUN touch src/main.rs && cargo build --release --locked

FROM ${RUNTIME_IMAGE}
# 以固定 uid 运行（和 ${PUID}:${PGID} 一起用时由 compose 覆盖），数据目录的属主按这个给。
WORKDIR /
COPY --from=build /src/target/release/personal-data /app/personal-data
ENV DATA_DIR=/data
EXPOSE 8090
USER 65532:65532
CMD ["/app/personal-data", "serve"]
