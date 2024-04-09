#syntax=docker/dockerfile:1.2
# ARG RUST_VERSION=1.75
FROM registry.access.redhat.com/ubi8/ubi:latest as build

# cache mounts below may already exist and owned by root
USER root
WORKDIR /root/

# Install Rust and setup environment
RUN yum groupinstall 'Development Tools' -y && \
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs >> sh.rustup.rs && \
    sh ./sh.rustup.rs -y && \
    curl -LO https://github.com/protocolbuffers/protobuf/releases/download/v26.1/protoc-26.1-linux-s390_64.zip && \
    unzip protoc-26.1-linux-s390_64.zip && \
    cp -r /root/include/google /usr/include


# Update PATH    
ENV PATH="/root/.cargo/bin:/root/bin/protoc:$PATH"

#RUN env | grep PATH

# Verify Cargo and Protoc Compiler Verison
RUN cargo --version && protoc --version

# Build influxdb3
COPY . /influxdb3
WORKDIR /influxdb3

ARG CARGO_INCREMENTAL=yes
ARG CARGO_NET_GIT_FETCH_WITH_CLI=false
ARG PROFILE=release
ARG FEATURES=aws,gcp,azure,jemalloc_replacing_malloc
ARG PACKAGE=influxdb3
ENV CARGO_INCREMENTAL=$CARGO_INCREMENTAL \
    CARGO_NET_GIT_FETCH_WITH_CLI=$CARGO_NET_GIT_FETCH_WITH_CLI \
    PROFILE=$PROFILE \
    FEATURES=$FEATURES \
    PACKAGE=$PACKAGE

RUN \
  --mount=type=cache,id=influxdb3_rustup,sharing=locked,target=/usr/local/rustup \
  --mount=type=cache,id=influxdb3_registry,sharing=locked,target=/usr/local/cargo/registry \
  --mount=type=cache,id=influxdb3_git,sharing=locked,target=/usr/local/cargo/git \
  --mount=type=cache,id=influxdb3_target,sharing=locked,target=/influxdb_iox/target \
    du -cshx /usr/local/rustup /usr/local/cargo/registry /usr/local/cargo/git /influxdb_iox/target && \
    cargo build --target-dir /influxdb3/target --package="$PACKAGE" --profile="$PROFILE" --no-default-features --features="$FEATURES" && \
    objcopy --compress-debug-sections "target/$PROFILE/$PACKAGE" && \
    cp "/influxdb3/target/$PROFILE/$PACKAGE" /root/$PACKAGE && \
    du -cshx /usr/local/rustup /usr/local/cargo/registry /usr/local/cargo/git /influxdb_iox/target


FROM debian:bookworm-slim

RUN apt update \
    && apt install --yes ca-certificates gettext-base libssl3 --no-install-recommends \
    && rm -rf /var/lib/{apt,dpkg,cache,log} \
    && groupadd --gid 1500 influxdb3 \
    && useradd --uid 1500 --gid influxdb3 --shell /bin/bash --create-home influxdb3

USER influxdb3

RUN mkdir ~/.influxdb3

ARG PACKAGE=influxdb3
ENV PACKAGE=$PACKAGE

COPY --from=build "/root/$PACKAGE" "/usr/bin/$PACKAGE"
COPY docker/entrypoint.sh /usr/bin/entrypoint.sh

EXPOSE 8080 8082

ENTRYPOINT ["/usr/bin/entrypoint.sh"]

CMD ["serve"]
