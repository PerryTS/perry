import { MessageChannel } from "node:worker_threads";
import { inspect } from "node:util";

const { port1, port2 } = new MessageChannel();

function state(label: string) {
  const output = inspect(port1);
  console.log(
    label,
    output.includes("active: true"),
    output.includes("refed: true"),
    output.includes("refed: false"),
  );
}

console.log("custom:", typeof (port1 as any)[inspect.custom]);
state("initial:");
port1.ref();
state("refed:");
port1.unref();
state("unrefed:");
port1.close();
port2.close();
