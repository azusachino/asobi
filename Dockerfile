FROM docker.io/library/rust:1-slim-bookworm AS build

WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates

RUN cargo build --locked --release -p asobi-server \
    && install -d -o 65532 -g 65532 /data

FROM azusachino.com/media-base:py3.14-trixie-r2

COPY --from=build --chown=65532:65532 /src/target/release/asobi-server /asobi-server
COPY --from=build --chown=65532:65532 /data /data

USER 65532:65532
VOLUME ["/data"]
EXPOSE 8300
ENTRYPOINT ["/asobi-server"]
CMD ["--listen", "0.0.0.0:8300", "--data-dir", "/data"]
