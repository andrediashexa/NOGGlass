# RouterOS 6, the device under test.
#
# Not the same product as RouterOS 7 with an older version number: the BGP
# configuration lives somewhere else entirely, under /routing/bgp/instance and
# /routing/bgp/peer rather than /routing/bgp/connection, and the output the
# driver has to read differs with it. This node exists to exercise the half of
# the driver that has never run against anything.
#
#   ether2 -> peer-a   ether3 -> peer-b   ether4 -> peer-c
/ip address
add address=192.0.2.1/30 interface=ether2
add address=192.0.2.5/30 interface=ether3
add address=192.0.2.9/30 interface=ether4
/ipv6 address
add address=2001:db8:0:1::1/64 interface=ether2 advertise=no
add address=2001:db8:0:2::1/64 interface=ether3 advertise=no
add address=2001:db8:0:3::1/64 interface=ether4 advertise=no
# Announce nothing: a looking glass router has no customers, and here it keeps
# this router's AS out of peer-c's AS_SET.
/routing filter
add chain=BGP-OUT action=discard
/routing bgp instance
set default as=64499 router-id=192.0.2.1
/routing bgp peer
add name=peer-a remote-address=192.0.2.2 remote-as=64496 out-filter=BGP-OUT address-families=ip
add name=peer-b remote-address=192.0.2.6 remote-as=64497 out-filter=BGP-OUT address-families=ip
add name=peer-c remote-address=192.0.2.10 remote-as=64498 out-filter=BGP-OUT address-families=ip
add name=never remote-address=192.0.2.126 remote-as=64511 out-filter=BGP-OUT address-families=ip
add name=peer-a-v6 remote-address=2001:db8:0:1::2 remote-as=64496 out-filter=BGP-OUT address-families=ipv6
add name=peer-b-v6 remote-address=2001:db8:0:2::2 remote-as=64497 out-filter=BGP-OUT address-families=ipv6
add name=peer-c-v6 remote-address=2001:db8:0:3::2 remote-as=64498 out-filter=BGP-OUT address-families=ipv6
