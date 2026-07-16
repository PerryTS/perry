import tls from "node:tls";
import net from "node:net";

const socket = new tls.TLSSocket(new net.Socket());
console.log("class:", socket instanceof tls.TLSSocket, socket instanceof net.Socket);
console.log("flags:", socket.encrypted, socket.authorized, socket.authorizationError);
console.log("negotiation:", socket.alpnProtocol, socket.servername, socket.getProtocol());
console.log("session:", socket.getSession(), socket.isSessionReused());
console.log("peer:", Object.keys(socket.getPeerCertificate()).length);
socket.destroy();
