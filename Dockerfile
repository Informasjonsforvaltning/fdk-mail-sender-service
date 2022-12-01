FROM rust:1.64 AS builder

WORKDIR /build

RUN apt-get update && apt-get install -y --no-install-recommends \
    clang

COPY ./ ./
RUN cargo build --release


FROM rust:1.64

ENV TZ=Europe/Oslo
RUN ln -snf /usr/share/zoneinfo/$TZ /etc/localtime && echo $TZ > /etc/timezone

COPY --from=builder /build/target/release/fdk-mail-sender-service /fdk-mail-sender-service

CMD ["/fdk-mail-sender-service"]
