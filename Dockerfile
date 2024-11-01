FROM rust:latest AS builder

WORKDIR /build

RUN apt-get update && apt-get install -y --no-install-recommends \
    clang

COPY ./ ./
RUN cargo build --release


FROM debian:bookworm-slim

ENV TZ=Europe/Oslo
RUN apt-get update && apt-get install -y libssl3 && rm -rf /var/lib/apt/lists/*
RUN ln -snf /usr/share/zoneinfo/$TZ /etc/localtime && echo $TZ > /etc/timezone

COPY --from=builder /build/target/release/fdk-mail-sender-service /fdk-mail-sender-service

CMD ["/fdk-mail-sender-service"]
