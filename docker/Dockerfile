FROM rust:latest

RUN cargo install monolith
RUN cargo install textpod

RUN mkdir /app
RUN mkdir /app/notes
WORKDIR /app

RUN apt update
RUN apt install nginx -y
COPY ./nginx.conf /etc/nginx/sites-enabled/default

COPY <<EOF /app/start.sh
#!/bin/bash
service nginx start
cd notes
textpod -p 3000
EOF

RUN chmod +x /app/start.sh

CMD ["/app/start.sh"]