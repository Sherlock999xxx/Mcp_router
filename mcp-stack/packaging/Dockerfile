FROM rust:1.75 as builder
WORKDIR /app
COPY . .
RUN cargo build --release -p mcp-router

FROM gcr.io/distroless/cc-debian12
COPY --from=builder /app/target/release/mcp-router /usr/local/bin/mcp-router
COPY config/router.toml /etc/mcp/router.toml
ENV RUST_LOG=info
EXPOSE 8848
ENTRYPOINT ["/usr/local/bin/mcp-router", "--config", "/etc/mcp/router.toml"]
