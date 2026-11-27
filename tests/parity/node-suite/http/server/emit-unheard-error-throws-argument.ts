// An unheard 'error' on a server throws the emitted argument itself.
import * as http from "node:http";

const server = http.createServer();

const err = new Error("boom");
try {
  server.emit("error", err);
  console.log("no throw");
} catch (caught) {
  console.log("same error:", caught === err, (caught as Error).message);
}

server.on("error", (e: Error) => console.log("heard:", e.message));
console.log("emit returned:", server.emit("error", err));
