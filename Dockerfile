FROM rust:1.75

WORKDIR /app
COPY . .

RUN cargo build --release

EXPOSE 3001
CMD ["./target/release/deplay-backend"]