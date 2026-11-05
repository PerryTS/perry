import { MessageChannel } from "node:worker_threads";

const { port1, port2 } = new MessageChannel();

function outcome(label: string, fn: () => void) {
  try {
    fn();
    console.log(label, "ok");
  } catch (error: any) {
    console.log(label, error?.name, error?.code ?? "");
  }
}

outcome("on null:", () => (port1 as any).on("message", null));
outcome("once number:", () => (port1 as any).once("message", 1));
outcome("addListener object:", () => (port1 as any).addListener("message", {}));
outcome("off undefined:", () => (port1 as any).off("message", undefined));
outcome(
  "addEventListener null:",
  () => port1.addEventListener("message", null as any),
);

port1.close();
port2.close();
