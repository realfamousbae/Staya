# Сборщик метрик этапа 1. Собирается в CI из корня репозитория:
#   docker build -f tools/probe-server/Dockerfile .
FROM rust:1.98.1-slim-bookworm@sha256:ff521445a372125ed4f76e1453a1f8098f2d05332d1601d30db1c1f62757e730 AS build
# Тулчейн образа уже 1.98.1; переменная отключает rust-toolchain.toml,
# чтобы rustup не докачивал rustfmt/clippy.
ENV RUSTUP_TOOLCHAIN=1.98.1
WORKDIR /src
COPY . .
RUN cargo build --release --locked -p probe-server

FROM gcr.io/distroless/cc-debian12:nonroot@sha256:9dac0a79194e45a7da0158a9c6da57b217585af0786db3845d1f0ec1a0dd182f
LABEL org.opencontainers.image.source="https://github.com/realfamousbae/Staya"
LABEL org.opencontainers.image.description="Staya stage 1 probe server (metrics only, no coordinates)"
LABEL org.opencontainers.image.licenses="AGPL-3.0-only"
COPY --from=build /src/target/release/probe-server /probe-server
USER nonroot
ENTRYPOINT ["/probe-server"]
