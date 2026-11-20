Closed the remaining Node HTTP/HTTPS behavioral-parity tail. Agent queue, socket,
session, and global-Agent lifecycle behavior now matches the pinned Node shard,
and HTTPS supports the remaining TLS identity, certificate, PKCS#12, SNI, keylog,
server/listener, and client-certificate paths exercised there. The issue's
first-25-per-API metric now reports 39 passes, 11 Node-oracle skips, and no Perry
failures.
