// An unhandled SIGTERM ends the program with status 128 + 15 while it waits on
// an idle listener.
import * as net from "node:net";

const server = net.createServer(() => {});
server.listen(0, () => console.log("listening"));
