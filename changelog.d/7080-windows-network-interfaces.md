Fixed `os.networkInterfaces()` returning an empty object on Windows. Perry now
enumerates live Windows adapters with Node-compatible names, addresses,
netmasks, MAC addresses, loopback flags, CIDRs, and IPv6 scope IDs.
