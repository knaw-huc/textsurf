# Build stage
FROM alpine:latest AS builder

RUN apk update && apk add cargo git

COPY . /usr/src/

WORKDIR /usr/src/

RUN cargo install --root /usr/ --path .

# Runtime stage
FROM alpine:latest

ARG UID=1000

RUN apk update && apk add runit sudo libgcc && adduser -u $UID -D user && mkdir -p /etc/service/textsurf

# Set to one to make the service writable instead of read-only. Note that the service itself does not offer any authentication!!!
ENV WRITABLE=0

# Unload time in seconds
ENV UNLOADTIME=600

# Set to 1 for debug output
ENV DEBUG=0

RUN mkdir -p /usr/src

# Copy the compiled binary from builder stage
COPY --from=builder /usr/bin/textsurf /usr/bin/textsurf

# Copy the run script
COPY etc/textsurf.run.sh /etc/service/textsurf/run

EXPOSE 8080
VOLUME /data

ENTRYPOINT ["runsvdir","-P","/etc/service"]

