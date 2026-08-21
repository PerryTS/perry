**Runtime: make `node:dgram` multicast usable by Bonjour/mDNS clients.**

UDP sockets now apply Node-compatible `reuseAddr` and `ipv6Only` options before bind, including the BSD/Apple `SO_REUSEPORT` behavior required for multiple mDNS listeners. Multicast membership, interface, TTL, and loopback controls operate on the live socket and report failures instead of silently accepting invalid configuration.

Closing a socket now wakes and joins its receive worker before emitting `close`, releasing shared ports promptly. Datagram callbacks also restore the AsyncLocalStorage context captured when the socket was bound. New host-network fixtures cover shared bind/cleanup, multicast loopback delivery with join/drop membership, and async-context propagation.
