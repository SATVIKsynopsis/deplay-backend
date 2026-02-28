pub const CPP_DOCKERFILE: &str = r#"
FROM gcc:13 AS build
WORKDIR /app
COPY . .
RUN make

FROM debian:bookworm-slim
WORKDIR /app
COPY --from=build /app/app app
CMD ["./app"]
"#;
