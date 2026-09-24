#!/usr/bin/env bash
# A one-node, real Ceph cluster for the ignored control-plane tests. MemStore is
# Ceph's in-memory OSD backend: no loop devices, mounts, or privileged Docker.
set -euo pipefail

if [[ $# -ne 2 || ( $1 != start && $1 != stop ) ]]; then
    echo "Usage: bash $0 <start|stop> <state-directory>" >&2
    exit 2
fi

action=$1
state=$2
if [[ $state != /* ]]; then
    echo "Ceph state directory must be an absolute path" >&2
    exit 2
fi

stop_cluster() {
    local daemon pid i
    for daemon in osd.0 mgr.x mon.a; do
        if [[ -f $state/$daemon.pid ]]; then
            read -r pid < "$state/$daemon.pid"
            if [[ $pid =~ ^[0-9]+$ && $pid -gt 1 ]]; then
                kill -TERM "$pid" 2>/dev/null || true
                for ((i = 0; i < 30; i++)); do
                    if ! kill -0 "$pid" 2>/dev/null; then
                        break
                    fi
                    sleep 0.1
                done
                if kill -0 "$pid" 2>/dev/null; then
                    kill -KILL "$pid" 2>/dev/null || true
                fi
            fi
        fi
    done
    # Never remove or replace a config belonging to another cluster.
    for file in ceph.conf ceph.client.admin.keyring; do
        if [[ -L /etc/ceph/$file && $(readlink "/etc/ceph/$file") == "$state/$file" ]]; then
            rm -- "/etc/ceph/$file"
        fi
    done
}

if [[ $action == stop ]]; then
    if [[ -f $state/owned ]]; then
        stop_cluster
    fi
    exit 0
fi

if [[ $(id -u) -ne 0 ]]; then
    echo "Ceph tests need a writable /etc/ceph (run as root inside the CI container)" >&2
    exit 1
fi
for binary in ceph-mon ceph-osd ceph-mgr ceph-authtool monmaptool ceph rbd; do
    command -v "$binary" >/dev/null || { echo "Missing Ceph binary: $binary" >&2; exit 1; }
done
if [[ -e $state || -L $state ]]; then
    echo "Refusing to reuse Ceph test state: $state" >&2
    exit 1
fi
for file in ceph.conf ceph.client.admin.keyring; do
    if [[ -e /etc/ceph/$file || -L /etc/ceph/$file ]]; then
        echo "Refusing to replace /etc/ceph/$file" >&2
        exit 1
    fi
done
mkdir -p -m 0700 -- "$state" /etc/ceph
: > "$state/owned"
trap 'stop_cluster; echo "Ceph startup failed; inspect $state/*.log" >&2' ERR

fsid=$(cat /proc/sys/kernel/random/uuid)
cat > "$state/ceph.conf" <<EOF
[global]
fsid = $fsid
mon host = 127.0.0.1:6789
ms bind msgr2 = false
ms bind msgr1 = true
auth cluster required = cephx
auth service required = cephx
auth client required = cephx
run dir = $state
log file = $state/\$name.log
pid file = $state/\$name.pid
admin socket = $state/\$name.asok
osd pool default size = 1
osd pool default min size = 1
osd crush chooseleaf type = 0

[mon.a]
mon data = $state/mon.a
mon data avail crit = 1

[mgr.x]
mgr data = $state/mgr.x

[osd.0]
osd data = $state/osd.0
osd objectstore = memstore
EOF

ceph-authtool --create-keyring "$state/bootstrap.keyring" --gen-key --name mon. --cap mon 'allow *'
ceph-authtool "$state/bootstrap.keyring" --gen-key --name client.admin \
    --cap mon 'allow *' --cap osd 'allow *' --cap mgr 'allow *' --cap mds 'allow *'
cp "$state/bootstrap.keyring" "$state/ceph.client.admin.keyring"
chmod 0600 "$state/bootstrap.keyring" "$state/ceph.client.admin.keyring"
monmaptool --create --add a 127.0.0.1:6789 --fsid "$fsid" "$state/monmap" >/dev/null
mkdir "$state/mon.a"
ceph-mon --conf "$state/ceph.conf" --id a --mkfs \
    --monmap "$state/monmap" --keyring "$state/bootstrap.keyring"
ln -s "$state/ceph.conf" /etc/ceph/ceph.conf
ln -s "$state/ceph.client.admin.keyring" /etc/ceph/ceph.client.admin.keyring
ceph-mon --id a --daemonize

wait_for() {
    local stage=$1
    shift
    local deadline=$((SECONDS + 90))
    until timeout 4 "$@" >/dev/null 2>&1; do
        if (( SECONDS >= deadline )); then
            echo "Timed out waiting for $stage (logs: $state/*.log)" >&2
            return 1
        fi
        sleep 1
    done
}
wait_for 'Ceph monitor' ceph status

mkdir "$state/mgr.x"
ceph auth get-or-create mgr.x mon 'allow profile mgr' osd 'allow *' mds 'allow *' \
    -o "$state/mgr.x/keyring"
ceph-mgr --id x --daemonize

osd_uuid=$(cat /proc/sys/kernel/random/uuid)
osd_id=$(ceph osd create "$osd_uuid")
if [[ $osd_id != 0 ]]; then
    echo "Expected first OSD ID 0, got $osd_id" >&2
    false
fi
mkdir "$state/osd.0"
ceph-authtool --create-keyring "$state/osd.0/keyring" --gen-key --name osd.0
ceph auth add osd.0 mon 'allow rwx' osd 'allow *' -i "$state/osd.0/keyring"
ceph-osd --id 0 --mkfs --osd-uuid "$osd_uuid"
ceph osd crush add osd.0 1 root=default host=ceph-test
ceph-osd --id 0 --daemonize
wait_for 'Ceph OSD' bash -c '[[ $(ceph osd stat) == *"1 up"* ]]'

for pool in templates volumes; do
    ceph osd pool create "$pool" 8
    ceph osd pool application enable "$pool" rbd
    rbd pool init "$pool"
    wait_for "RBD pool $pool" rbd ls --pool "$pool"
done
ceph auth get-or-create client.libvirt mon 'allow r' osd 'allow rwx' \
    -o "$state/ceph.client.libvirt.keyring"
trap - ERR
echo "Ceph test backend ready: $state (stop with: bash ci/ceph-test.sh stop $state)"
