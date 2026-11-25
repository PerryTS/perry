import http from "node:http";
import net from "node:net";

// `close(callback)` calls back after the last connection has closed: an
// in-flight response, a half-received request and an upgraded socket all hold
// it, and an idle connection does not.
const REQUEST = "GET / HTTP/1.1\r\nHost: localhost\r\n";

function run(
  name: string,
  request: string,
  onConnected: (server: http.Server, client: net.Socket) => void,
  onRequest: (server: http.Server, res: http.ServerResponse) => void = () => {},
  onUpgrade?: (server: http.Server, socket: net.Socket) => void,
): Promise<void> {
  return new Promise((resolve) => {
    const server = http.createServer((_req, res) => onRequest(server, res));
    if (onUpgrade) server.on("upgrade", (_req, socket) => onUpgrade(server, socket as net.Socket));
    let received = "";
    let pending = 2;
    const done = () => {
      if (--pending > 0) return;
      setImmediate(() => {
        console.log(name, "client got:", received.split("\r\n")[0] || "(nothing)");
        resolve();
      });
    };
    server.on("close", () => {
      console.log(name, "close event", server.listening);
      done();
    });
    server.listen(0, "127.0.0.1", () => {
      const { port } = server.address() as net.AddressInfo;
      const client = net.connect(port, "127.0.0.1");
      server.once("connection", () => setImmediate(() => {
        if (request) client.write(request);
        onConnected(server, client);
      }));
      client.on("data", (chunk) => (received += chunk.toString()));
      client.on("error", () => {});
      client.on("close", done);
    });
  });
}

function close(name: string, server: http.Server) {
  console.log(name, "close called");
  server.close(() => console.log(name, "callback"));
}

await run("in flight", REQUEST + "Connection: close\r\n\r\n", () => {}, (server, res) => {
  close("in flight", server);
  setTimeout(() => {
    console.log("in flight", "response ends");
    res.end("done");
  }, 100);
});

await run("half received", REQUEST, (server, client) => {
  setTimeout(() => {
    close("half received", server);
    setTimeout(() => {
      console.log("half received", "client ends");
      client.destroy();
    }, 100);
  }, 50);
});

await run(
  "upgraded",
  REQUEST + "Connection: Upgrade\r\nUpgrade: echo\r\n\r\n",
  () => {},
  undefined,
  (server, socket) => {
    close("upgraded", server);
    setTimeout(() => {
      console.log("upgraded", "socket ends");
      socket.end("bye\r\n");
    }, 100);
  },
);

await run("idle", "", (server) => close("idle", server));

await run("forced", REQUEST + "\r\n", () => {}, (server) => {
  close("forced", server);
  setTimeout(() => {
    console.log("forced", "closeAllConnections");
    server.closeAllConnections();
  }, 100);
});
