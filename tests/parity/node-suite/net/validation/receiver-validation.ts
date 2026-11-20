import * as net from "node:net";

const cases: [string, Function][] = [
  ["socket address", net.Socket.prototype.address],
  ["socket ref", net.Socket.prototype.ref],
  ["server address", net.Server.prototype.address],
  ["server close", net.Server.prototype.close],
];

for (const [label, method] of cases) {
  try {
    method.call({});
    console.log(label, "OK");
  } catch (error: any) {
    console.log(label, error.name, error.code);
  }
}
