# Use RedHat UBI Image
FROM registry.access.redhat.com/ubi8/ubi:latest as build

# Setup root
USER root
WORKDIR /root/

# Set Go ENV VAR
ENV GO111MODULE=on

# Setup environment for influx build environment
RUN yum update -y && \
    yum groupinstall 'Development Tools' -y && \
    yum install bison clang golang protobuf -y && \ 
    curl -s -S -L https://raw.githubusercontent.com/moovweb/gvm/master/binscripts/gvm-installer >> gvminstall.sh && \
    chmod +x /root/gvminstall.sh && \
    bash /root/gvminstall.sh && \
    source /root/.gvm/scripts/gvm && \
    gvm install go1.18 && \
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs >> sh.rustup.rs && \
    sh ./sh.rustup.rs -y && \
    curl -LO https://github.com/protocolbuffers/protobuf/releases/download/v26.1/protoc-26.1-linux-s390_64.zip && \
    unzip protoc-26.1-linux-s390_64.zip && \
    cp -r /root/include/google /usr/include && \
    ln -s /root/bin/protoc /usr/bin/protoc

# Update PATH    
ENV PATH="/root/.cargo/bin:$PATH"

# Verify Cargo and Protoc Compiler Verison
RUN cargo --version && protoc --version

# Clone Influx Git repo
RUN git clone https://github.com/influxdata/influxdb.git

# Move to Influx Git dir
WORKDIR /root/influxdb/

# Build Influx v2.7.5
RUN git checkout v2.7.5 && \ 
    make
      
FROM quayreg1.fpet.pokprv.stglabs.ibm.com/fmitaro/debian:bookworm-slim AS dependency-base

ENV DEBIAN_FRONTEND noninteractive

RUN apt-get update \
    && apt-get install -y \
    ca-certificates \
    tzdata \
    && apt-get clean autoclean \
    && apt-get autoremove --yes \
    && rm -rf /var/lib/{apt,dpkg,cache,log}

# NOTE: We separate these two stages so we can run the above
# quickly in CI, in case of flaky failure.
FROM dependency-base

EXPOSE 8086

COPY --from=build /root/influxdb/cmd/influxd /usr/bin/influxd
COPY --from=build /root/influxdb/docker/influxd/entrypoint.sh /entrypoint.sh

ENTRYPOINT ["/entrypoint.sh"]
CMD ["influxd"]
