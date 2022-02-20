#!/bin/bash

set -euo pipefail

cd "$(dirname "$0")"

TMP="tmp-cluster-controller-build"
REMOTE="registry.acl.fi/public/virt-controller"
DATE="$(date +%Y%m%d-%H%M%S)"

if [ "$(git status --porcelain)" == "" ]; then
    CLEAN=true
else
    CLEAN=false
fi

CACHE_DIR=".podman_cache"

if [[ -d ${CACHE_DIR} ]]; then 
    [[ -d ${CACHE_DIR}/registry ]] || mkdir ${CACHE_DIR}/registry
    [[ -d ${CACHE_DIR}/build ]] || mkdir ${CACHE_DIR}/build

    PODMAN_BUILD_OPTS="-v ${PWD}/${CACHE_DIR}/registry:/usr/local/cargo/registry:Z -v ${PWD}/${CACHE_DIR}/build:/usr/src/cluster-controller/target:Z"
fi

podman build ${PODMAN_BUILD_OPTS:-} -t "$TMP" .
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
