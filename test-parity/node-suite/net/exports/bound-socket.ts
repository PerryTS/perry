import * as net from "node:net";

const BoundSocket = (net as any).BoundSocket;
console.log("export:", typeof BoundSocket, BoundSocket?.length);

if (typeof BoundSocket === "function") {
  for (const value of [null, 1, "x"] as any[]) {
    try {
      new BoundSocket(value);
      console.log("construct", String(value), "OK");
    } catch (error: any) {
      console.log("construct", String(value), error.name, error.code);
    }
  }
}
