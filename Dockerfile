FROM rust:latest AS builder

WORKDIR /app
COPY . .
COPY ./.env .
RUN cargo build --release -v

CMD ["/app/target/release/taiko-preconfer"]
