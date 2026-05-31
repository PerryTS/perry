import * as dgram from "node:dgram";

function codeOf(fn) {
  try {
    fn();
    return "none";
  } catch (error) {
    return error.code;
  }
}

function describe(value) {
  if (value === undefined) return "undefined";
  if (value === null) return "null";
  return `${typeof value}:${value}`;
}

const socket = dgram.createSocket({ type: "udp4", reuseAddr: true });
await new Promise((resolve) => {
  socket.bind(0, "0.0.0.0", () => resolve());
});

console.log("addMembership:", describe(socket.addMembership("224.0.0.114")));
console.log("dropMembership:", describe(socket.dropMembership("224.0.0.114")));
console.log(
  "addSourceSpecificMembership:",
  describe(socket.addSourceSpecificMembership("127.0.0.1", "232.0.0.114")),
);
console.log(
  "dropSourceSpecificMembership:",
  describe(socket.dropSourceSpecificMembership("127.0.0.1", "232.0.0.114")),
);
console.log("missing membership:", codeOf(() => socket.addMembership()));
console.log("bad membership address:", codeOf(() => socket.addMembership(1)));
console.log("missing ssm source:", codeOf(() => socket.addSourceSpecificMembership()));
console.log(
  "missing ssm group:",
  codeOf(() => socket.addSourceSpecificMembership("127.0.0.1")),
);
console.log(
  "bad ssm source:",
  codeOf(() => socket.addSourceSpecificMembership(1, "232.0.0.114")),
);
console.log(
  "bad ssm group:",
  codeOf(() => socket.addSourceSpecificMembership("127.0.0.1", 1)),
);
socket.close();
