# RouterOS 7, the device under test.
#
# Same addressing as the FRR reference node, so the peers need no change:
#   ether2 -> peer-a   ether3 -> peer-b   ether4 -> peer-c
# (ether1 is the management interface vrnetlab wires up.)
#
# It announces nothing. A looking glass router has no customers, and here it
# also keeps this router's own AS out of peer-c's AS_SET.
/ip address
add address=192.0.2.1/30 interface=ether2
add address=192.0.2.5/30 interface=ether3
add address=192.0.2.9/30 interface=ether4
/ipv6 address
add address=2001:db8:0:1::1/64 interface=ether2 advertise=no
add address=2001:db8:0:2::1/64 interface=ether3 advertise=no
add address=2001:db8:0:3::1/64 interface=ether4 advertise=no
# Announce nothing. Without this RouterOS does what eBGP says and passes what
# it learns from one peer to the others, which puts this router's own AS inside
# peer-c's AS_SET — and the aggregate then comes back as a loop and is dropped.
# The most interesting route in the lab was missing until this was added.
/routing filter rule
add chain=BGP-OUT rule="reject"
/routing bgp connection
add name=peer-a remote.address=192.0.2.2 remote.as=64496 local.address=192.0.2.1 local.role=ebgp as=64499 router-id=192.0.2.1 address-families=ip output.filter-chain=BGP-OUT
add name=peer-b remote.address=192.0.2.6 remote.as=64497 local.address=192.0.2.5 local.role=ebgp as=64499 router-id=192.0.2.1 address-families=ip output.filter-chain=BGP-OUT
add name=peer-c remote.address=192.0.2.10 remote.as=64498 local.address=192.0.2.9 local.role=ebgp as=64499 router-id=192.0.2.1 address-families=ip output.filter-chain=BGP-OUT
add name=never remote.address=192.0.2.126 remote.as=64511 local.address=192.0.2.1 local.role=ebgp as=64499 router-id=192.0.2.1 address-families=ip output.filter-chain=BGP-OUT
add name=peer-a-v6 remote.address=2001:db8:0:1::2 remote.as=64496 local.address=2001:db8:0:1::1 local.role=ebgp as=64499 router-id=192.0.2.1 address-families=ipv6 output.filter-chain=BGP-OUT
add name=peer-b-v6 remote.address=2001:db8:0:2::2 remote.as=64497 local.address=2001:db8:0:2::1 local.role=ebgp as=64499 router-id=192.0.2.1 address-families=ipv6 output.filter-chain=BGP-OUT
add name=peer-c-v6 remote.address=2001:db8:0:3::2 remote.as=64498 local.address=2001:db8:0:3::1 local.role=ebgp as=64499 router-id=192.0.2.1 address-families=ipv6 output.filter-chain=BGP-OUT
