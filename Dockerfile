# syntax=docker/dockerfile:1.7
FROM rust:1.88.0-bookworm AS build

WORKDIR /workspace
ENV CARGO_INCREMENTAL=0

COPY Cargo.toml Cargo.lock ./
COPY apps/server ./apps/server
COPY crates ./crates
COPY db ./db

RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/workspace/target,sharing=locked \
    cargo build --locked --release \
      --package sprout-server \
      --package sprout-validation-cli \
    && install -D -m 0755 target/release/sprout-server /out/sprout-server \
    && install -D -m 0755 \
      target/release/sprout-validation-crypto \
      /out/sprout-validation-crypto

FROM debian:bookworm-slim AS validation

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
      bash ca-certificates curl jq postgresql-client \
    && rm -rf /var/lib/apt/lists/*

COPY --from=build /out/sprout-validation-crypto /usr/local/bin/sprout-validation-crypto
COPY scripts/validation/roundtrip.sh /usr/local/bin/sprout-validation-roundtrip

RUN chmod 0755 /usr/local/bin/sprout-validation-roundtrip

ENTRYPOINT ["/usr/local/bin/sprout-validation-roundtrip"]

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates libssl3 \
    && rm -rf /var/lib/apt/lists/*

COPY --from=build --chown=65532:65532 /out/sprout-server /usr/local/bin/sprout-server
COPY --from=build --chown=65532:65532 /workspace/db/migrations /opt/sprout/migrations

RUN install -d -o 65532 -g 65532 -m 0700 \
      /var/lib/sprout /var/lib/sprout/blobs /var/lib/sprout/archives

ENV RUST_LOG=info
ENV SPROUT_MIGRATIONS_DIR=/opt/sprout/migrations
ENV SPROUT_BLOB_DIR=/var/lib/sprout/blobs
ENV SPROUT_ARCHIVE_DIR=/var/lib/sprout/archives
WORKDIR /var/lib/sprout
EXPOSE 8080
USER 65532:65532
ENTRYPOINT ["/usr/local/bin/sprout-server"]
