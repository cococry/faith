# syntax=docker/dockerfile:1
ARG BASE_IMAGE=debian:bookworm-slim
FROM ${BASE_IMAGE} AS build
RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential libssl-dev libpq-dev ca-certificates && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY third_party ./third_party
COPY faith-proto ./faith-proto
COPY faith-server ./faith-server
RUN cd faith-proto && cc -o nob nob.c && ./nob && \
    cd ../faith-server && cc -o nob nob.c && ./nob && strip build/faith-server

FROM ${BASE_IMAGE} AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends \
    libssl3 libpq5 postgresql-client openssl ca-certificates && rm -rf /var/lib/apt/lists/* && \
    groupadd --gid 10001 faith && useradd --uid 10001 --gid 10001 --no-create-home faith
WORKDIR /app
COPY --from=build /src/faith-server/build/faith-server /app/faith-server
COPY faith-db/migrations /app/migrations
COPY deploy/migrate.sh /app/migrate.sh
USER 10001:10001
EXPOSE 4433
STOPSIGNAL SIGTERM
HEALTHCHECK --interval=15s --timeout=5s --start-period=10s --retries=3 \
    CMD timeout 3 openssl s_client -connect 127.0.0.1:4433 </dev/null >/dev/null 2>&1 || exit 1
ENTRYPOINT ["/app/faith-server"]
