#!/bin/bash

set -euo pipefail

TMP="tmp-cluster-controller-build"
REMOTE="registry.acl.fi/public/virt-controller"
DATE="$(date +%Y%m%d-%H%M%S)"

podman build -t "$TMP" .
version="$(podman run --rm tmp-cluster-controller-build cluster-controller --version)"

podman tag "$TMP" "$REMOTE:$version"
podman tag "$TMP" "$REMOTE:$DATE"
podman tag "$TMP" "$REMOTE:latest"

podman push "$REMOTE:$version"
podman push "$REMOTE:$DATE"
podman push "$REMOTE:latest"
