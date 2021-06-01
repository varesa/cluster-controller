# Build

FROM docker.io/library/rust:latest as builder

WORKDIR /usr/src/cluster-deployer
COPY . .

RUN cargo install --path .


# Run

FROM registry.access.redhat.com/ubi8/ubi
COPY --from=builder /usr/local/cargo/bin/cluster-controller /usr/local/bin/cluster-controller

