# Build

FROM docker.io/library/rust:latest as builder

WORKDIR /usr/src/cluster-deployer
COPY Cargo.toml build.rs .

RUN apt-get update && \
    apt-get install -y librados-dev librbd-dev libvirt-dev && \
    rm -rf /var/lib/apt/lists/*

# Only install dependencies
RUN mkdir src && \
    echo "fn main() {}" >> src/main.rs && \
    cargo build --release

COPY . .
RUN cargo install --path .

# Run

FROM centos:8
RUN dnf install -y libvirt-libs librbd1 librados2
COPY --from=builder /usr/local/cargo/bin/cluster-controller /usr/local/bin/cluster-controller

