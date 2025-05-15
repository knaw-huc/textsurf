FROM alpine:latest

RUN apk update && apk add cargo runit sudo git && adduser -u 1000 -D user && mkdir -p /usr/src /etc/service/textsurf

COPY . /usr/src/

# Set to one to make the service writable instead of read-only. Note that the service itself does not offer any authentication!!!
ENV WRITABLE=0

# Unload time in seconds
ENV UNLOADTIME=600

# Set to 1 for debug output
ENV DEBUG=0

WORKDIR /usr/src/

RUN cargo install --root /usr/ --path . &&\
    mv etc/textsurf.run.sh /etc/service/textsurf/run

EXPOSE 80
VOLUME /data

ENTRYPOINT ["runsvdir","-P","/etc/service"]
