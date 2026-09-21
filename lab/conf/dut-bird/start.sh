#!/bin/sh
# Addresses first, then the daemon: BIRD refuses a neighbour it has no address
# to reach.
#
# The wait is not optional. containerlab creates the veth pairs after the
# container is already running, so configuring at once finds no interfaces and
# BIRD comes up with every session Idle — which looks like a routing problem
# and is not one.
set -e

wait_for() {
    interface="$1"
    attempt=0
    while [ ! -e "/sys/class/net/$interface" ]; do
        attempt=$((attempt + 1))
        if [ "$attempt" -gt 120 ]; then
            echo "$interface never appeared" >&2
            return 1
        fi
        sleep 0.5
    done
}

for interface in eth1 eth2 eth3; do
    wait_for "$interface"
done

ip address add 192.0.2.1/30 dev eth1
ip address add 192.0.2.5/30 dev eth2
ip address add 192.0.2.9/30 dev eth3
ip -6 address add 2001:db8:0:1::1/64 dev eth1 nodad
ip -6 address add 2001:db8:0:2::1/64 dev eth2 nodad
ip -6 address add 2001:db8:0:3::1/64 dev eth3 nodad

bird -c /etc/bird.conf
exec /usr/sbin/sshd -D -e
