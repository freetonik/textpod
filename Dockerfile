FROM rust:latest

RUN cargo install monolith
RUN cargo install textpod

WORKDIR /app/notes

HEALTHCHECK --interval=60s --retries=3 --timeout=1s \
CMD curl -f http://localhost:3000/ || exit 1

ENTRYPOINT ["textpod"]
CMD ["-p", "3000", "-l", "0.0.0.0"]