import * as net from "node:net";
const socket = new net.Socket();
socket.on("error", () => console.log("error"));
socket.on("close", () => console.log("close"));
socket.cork();
console.log("accepted", socket.write("pending", (error) => console.log("write", !!error)));
socket.end((error) => console.log("end", !!error));
