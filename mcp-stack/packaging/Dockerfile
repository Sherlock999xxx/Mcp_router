FROM gcr.io/distroless/cc-debian12
WORKDIR /app
COPY target/release/mcp-router /app/mcp-router
COPY config /app/config
COPY gui /app/gui
COPY migrations /app/migrations
ENV RUST_LOG=info
CMD ["/app/mcp-router", "--config", "config/router.toml"]
