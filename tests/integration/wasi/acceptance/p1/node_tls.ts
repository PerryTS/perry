// The TLS-consuming modules (tls, https, net, dns) link into the runtime and
// expose their surface; nothing connects, since sockets are unsupported here.
import * as tls from "node:tls";
import * as https from "node:https";
import * as net from "node:net";

console.log(typeof tls.connect, typeof tls.createServer, typeof https.request);
console.log(net.isIP("127.0.0.1"), net.isIP("::1"), net.isIP("example.test"));
console.log(tls.DEFAULT_MIN_VERSION, typeof tls.rootCertificates);
