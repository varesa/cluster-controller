#!/bin/bash

set -euo pipefail

if [[ -n "${1:-}" ]]; then
    OVN_INSTALL_VERSION="$1"
else
    echo "Usage: $0 <version>"
    echo "e.g. $0 24.03.5-14.el9s.x86_64"
    exit 1
fi
cd "$(dirname "$0")"

TMP="tmp-ovn-build"
REMOTE="registry.acl.fi/public/ovn"
DATE="$(date +%Y%m%d-%H%M%S)"
HASH="$(git rev-parse HEAD)"

if [ "$(git status --porcelain)" == "" ]; then
    CLEAN=true
else
    CLEAN=false
fi

podman build ${PODMAN_BUILD_OPTS:-} -f Containerfile.ovn --build-arg OVN_INSTALL_VERSION=$OVN_INSTALL_VERSION -t "$TMP" .

[ "$CLEAN" == "true" ] && podman tag "$TMP" "$REMOTE:$OVN_INSTALL_VERSION-$DATE-$HASH"
podman tag "$TMP" "$REMOTE:$OVN_INSTALL_VERSION-$DATE"
podman tag "$TMP" "$REMOTE:$OVN_INSTALL_VERSION-latest"

[ "$CLEAN" == "true" ] && podman push "$REMOTE:$OVN_INSTALL_VERSION-$DATE-$HASH"
podman push "$REMOTE:$OVN_INSTALL_VERSION-$DATE"
podman push "$REMOTE:$OVN_INSTALL_VERSION-latest"
