import { BlockList, Socket } from "node:net";

const cases: [string, any][] = [
  ["objectMode", { objectMode: true }],
  ["readableObjectMode", { readableObjectMode: true }],
  ["writableObjectMode", { writableObjectMode: true }],
  ["blockList", { blockList: {} }],
  ["typeOfService string", { typeOfService: "1" }],
  ["typeOfService range", { typeOfService: 256 }],
];

for (const [label, options] of cases) {
  try {
    new Socket(options).destroy();
    console.log(label, "OK");
  } catch (error: any) {
    console.log(label, error.name, error.code);
  }
}

const socket = new Socket({
  allowHalfOpen: 1 as any,
  blockList: new BlockList(),
  typeOfService: 16,
});
console.log(
  "accepted:",
  socket.allowHalfOpen,
  typeof (socket as any).getTypeOfService,
  typeof (socket as any).getTypeOfService === "function"
    ? (socket as any).getTypeOfService()
    : undefined,
);
socket.destroy();
