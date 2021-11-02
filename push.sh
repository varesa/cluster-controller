#!/bin/bash

set -euo pipefail

TMP="tmp-cluster-controller-build"
REMOTE="registry.acl.fi/public/virt-controller"
DATE="$(date +%Y%m%d-%H%M%S)"

if [ "$(git status --porcelain)" == "" ]; then
    CLEAN=true
else
    CLEAN=false
fi

podman build -t "$TMP" .
version="$(podman run --rm tmp-cluster-controller-build cluster-controller --version)"

[ "$CLEAN" == "true" ] && podman tag "$TMP" "$REMOTE:$version"
podman tag "$TMP" "$REMOTE:$DATE"
podman tag "$TMP" "$REMOTE:latest"

[ "$CLEAN" == "true" ] && podman push "$REMOTE:$version"
podman push "$REMOTE:$DATE"
podman push "$REMOTE:latest"

if [ "$CLEAN" == "true" ]; then
    IMG="$REMOTE:$version"
else
    IMG="$REMOTE:$DATE"
fi

kubectl set image \
    -n virt-controller \
    deployment/cluster-controller \
    cluster-controller="$IMG"
