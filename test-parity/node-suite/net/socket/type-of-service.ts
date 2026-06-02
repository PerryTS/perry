import * as net from "node:net";

function firstLine(err: any): string {
  return String(err.message).split("\n")[0];
}

function logCall(label: string, fn: () => unknown) {
  try {
    console.log(label, "OK", String(fn()));
  } catch (err: any) {
    console.log(label, "THROW", err.name, err.code, "|", firstLine(err));
  }
}

const socket = new net.Socket();

console.log(
  "socket TOS methods:",
  typeof (socket as any).getTypeOfService,
  typeof (socket as any).setTypeOfService,
);
logCall("socket TOS default", () => (socket as any).getTypeOfService());
logCall("socket set TOS self", () => (socket as any).setTypeOfService(16) === socket);
logCall("socket TOS after set", () => (socket as any).getTypeOfService());

for (const value of [-1, 256, 1.5, NaN, "16"] as any[]) {
  logCall(`socket set TOS ${String(value)}`, () => {
    (socket as any).setTypeOfService(value);
    return "NO THROW";
  });
}

socket.destroy();
