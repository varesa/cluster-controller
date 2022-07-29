# Build

FROM docker.io/library/rust:buster as builder

WORKDIR /usr/src/cluster-controller

# Clang is required for virt-sys bindgen
RUN apt-get update && \
    apt-get install -y clang llvm libclang-dev librados-dev librbd-dev libvirt-dev && \
    rm -rf /var/lib/apt/lists/*

# Only install dependencies
COPY Cargo.toml Cargo.lock build.rs .
RUN mkdir src && \
    echo "fn main() {}" >> src/main.rs && \
    cargo build --release

COPY . .
RUN cargo install --path .

# Run

FROM docker.io/library/rockylinux:8

RUN dnf update -y && \
    dnf install -y centos-release-ceph-pacific && \
    dnf install -y libvirt-libs librbd1 librados2 iproute

COPY --from=builder /usr/local/cargo/bin/cluster-controller /usr/local/bin/cluster-controller

