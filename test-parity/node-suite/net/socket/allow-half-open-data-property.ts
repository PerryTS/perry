import net from "node:net";

for (const [label, options] of [
  ["default", {}], ["true", { allowHalfOpen: true }],
  ["false", { allowHalfOpen: false }], ["string", { allowHalfOpen: "x" }],
  ["zero", { allowHalfOpen: 0 }],
] as const) {
  const socket: any = new net.Socket(options as any);
  console.log(label, JSON.stringify(Object.getOwnPropertyDescriptor(socket, "allowHalfOpen")));
  for (const value of [true, false, "x", 0, null, undefined]) {
    socket.allowHalfOpen = value;
    console.log("write", typeof socket.allowHalfOpen, String(socket.allowHalfOpen));
  }
  console.log("delete", delete socket.allowHalfOpen,
    Object.hasOwn(socket, "allowHalfOpen"), String(socket.allowHalfOpen));
  socket.destroy();
}
