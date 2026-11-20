import { MessageChannel } from "node:worker_threads";

const { port1, port2 } = new MessageChannel();
const dummy = () => {};

function outcome(fn: () => unknown): string {
  try {
    fn();
    return "ok";
  } catch (error: any) {
    return `${error?.name}:${error?.code ?? ""}`;
  }
}

function exercise(port: any): string {
  return [
    outcome(() => port.on("message", dummy)),
    outcome(() => port.off("message", dummy)),
    outcome(() => port.addListener("message", dummy)),
    outcome(() => port.removeListener("message", dummy)),
  ].join(",");
}

port1.on("message", dummy);
port1.close(() => {
  console.log("closed port:", exercise(port1));
  console.log("closed peer:", exercise(port2));
  port2.close();
});
console.log("closing port:", exercise(port1));
console.log("closing peer:", exercise(port2));
