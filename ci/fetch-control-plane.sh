#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 2 || -z ${1:-} || -z ${2:-} ]]; then
    echo "Usage: bash $0 <destination> <version>" >&2
    exit 1
fi

destination="$1"
version="$2"
url="https://github.com/kubernetes-sigs/controller-tools/releases/download/envtest-${version}/envtest-${version}-linux-amd64.tar.gz"

workdir=$(mktemp -d)

curl --fail --silent --show-error --location --proto '=https' --proto-redir '=https' \
    --output "$workdir/envtest.tar.gz" "$url"

mkdir "$workdir/bin"
tar xzf "$workdir/envtest.tar.gz" \
    --directory "$workdir/bin" --strip-components=2 --no-same-owner --no-same-permissions \
    controller-tools/envtest/etcd controller-tools/envtest/kube-apiserver
for binary in etcd kube-apiserver; do
    if [[ ! -f "$workdir/bin/$binary" || ! -x "$workdir/bin/$binary" ]]; then
        echo "archive is missing executable $binary." >&2
        exit 1
    fi
done

mkdir -p -- "$destination"
install -m 0755 -- "$workdir/bin/etcd" "$workdir/bin/kube-apiserver" "$destination/"
rm -rf "${workdir}"