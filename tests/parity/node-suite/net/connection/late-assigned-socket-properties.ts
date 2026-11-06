import * as net from "net";
import * as nodeNet from "node:net";

const server = net.createServer((socket) => socket.end());
const clients: net.Socket[] = [];
const deadline = setTimeout(() => {
  console.log("deadline");
  for (const socket of clients) socket.destroy();
  server.close();
  process.exitCode = 1;
}, 4000);

try {
  await new Promise<void>((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  const port = (server.address() as any).port;
  await new Promise<void>((resolve, reject) => {
    const socket = net.connect(port, "127.0.0.1", () => {
      console.log("direct-connect-captured:", socket.stableProbe);
      socket.destroy();
      resolve();
    });
    clients.push(socket);
    socket.once("error", reject);
    let reads = 0;
    Object.defineProperty(socket, "stableProbe", {
      configurable: true,
      get() { reads++; return false; },
    });
    console.log("direct-connect-getter:", socket.stableProbe, reads);
    Object.defineProperty(socket, "stableProbe", { value: true, writable: true });
    console.log("direct-connect-data:", socket.stableProbe, reads);
    socket.stableProbe = false;
    console.log("direct-connect-write:", socket.stableProbe, reads);
    socket.stableProbe = true;
    const method = socket.setNoDelay;
    console.log("direct-connect-method:", typeof method, socket.setNoDelay() === socket);
    try {
      socket.stableProbe();
    } catch (error) {
      console.log("direct-connect-call:", error instanceof TypeError);
    }
  });
  await new Promise<void>((resolve, reject) => {
    const socket = nodeNet.createConnection(port, "127.0.0.1", () => {
      console.log("direct-create-captured:", socket.stableProbe);
      socket.destroy();
      resolve();
    });
    clients.push(socket);
    socket.once("error", reject);
    let reads = 0;
    Object.defineProperty(socket, "stableProbe", {
      configurable: true,
      get() { reads++; return false; },
    });
    console.log("direct-create-getter:", socket.stableProbe, reads);
    Object.defineProperty(socket, "stableProbe", { value: true, writable: true });
    console.log("direct-create-data:", socket.stableProbe, reads);
    socket.stableProbe = false;
    console.log("direct-create-write:", socket.stableProbe, reads);
    socket.stableProbe = true;
    const method = socket.setNoDelay;
    console.log("direct-create-method:", typeof method, socket.setNoDelay() === socket);
    try {
      socket.stableProbe();
    } catch (error) {
      console.log("direct-create-call:", error instanceof TypeError);
    }
  });
  await new Promise<void>((resolve, reject) => {
    let socket: any;
    socket = net.connect(port, "127.0.0.1", () => {
      console.log("late-connect-captured:", socket.stableProbe);
      socket.destroy();
      resolve();
    });
    clients.push(socket);
    socket.once("error", reject);
    let reads = 0;
    Object.defineProperty(socket, "stableProbe", {
      configurable: true,
      get() { reads++; return false; },
    });
    console.log("late-connect-getter:", socket.stableProbe, reads);
    Object.defineProperty(socket, "stableProbe", { value: true, writable: true });
    console.log("late-connect-data:", socket.stableProbe, reads);
    socket.stableProbe = false;
    console.log("late-connect-write:", socket.stableProbe, reads);
    socket.stableProbe = true;
    const method = socket.setNoDelay;
    console.log("late-connect-method:", typeof method, socket.setNoDelay() === socket);
    try {
      socket.stableProbe();
    } catch (error) {
      console.log("late-connect-call:", error instanceof TypeError);
    }
    {
      const socket = { stableProbe: "shadow" };
      console.log("shadow:", socket.stableProbe);
    }
  });
  await new Promise<void>((resolve, reject) => {
    let socket: any;
    socket = nodeNet.createConnection(port, "127.0.0.1", () => {
      console.log("late-create-captured:", socket.stableProbe);
      socket.destroy();
      resolve();
    });
    clients.push(socket);
    socket.once("error", reject);
    let reads = 0;
    Object.defineProperty(socket, "stableProbe", {
      configurable: true,
      get() { reads++; return false; },
    });
    console.log("late-create-getter:", socket.stableProbe, reads);
    Object.defineProperty(socket, "stableProbe", { value: true, writable: true });
    console.log("late-create-data:", socket.stableProbe, reads);
    socket.stableProbe = false;
    console.log("late-create-write:", socket.stableProbe, reads);
    socket.stableProbe = true;
    const method = socket.setNoDelay;
    console.log("late-create-method:", typeof method, socket.setNoDelay() === socket);
    try {
      socket.stableProbe();
    } catch (error) {
      console.log("late-create-call:", error instanceof TypeError);
    }
    {
      const socket = { stableProbe: "shadow" };
      console.log("shadow:", socket.stableProbe);
    }
  });
  await new Promise<void>((resolve, reject) => {
    const socket = nodeNet.connect(port, "127.0.0.1").setNoDelay();
    clients.push(socket);
    socket.once("error", reject);
    socket.once("connect", () => {
      console.log("chain:", typeof socket.setNoDelay);
      socket.destroy();
      resolve();
    });
  });
} finally {
  clearTimeout(deadline);
  for (const socket of clients) socket.destroy();
  await new Promise<void>((resolve) => server.close(() => resolve()));
}
