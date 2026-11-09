import { MessageChannel, Worker } from "node:worker_threads";

process.chdir("tests/parity/node-suite/worker_threads/worker-lifecycle");
const worker = new Worker("./eventemitter-worker.cjs");
const events: string[] = [];

function outcome(label: string, fn: () => void) {
  try {
    fn();
    console.log(label, "ok");
  } catch (error: any) {
    console.log(label, error.name, error.code, error.message);
  }
}

outcome("typed on null:", () => worker.on("unknown", null as any));
outcome("typed once number:", () => worker.once("unknown", 1 as any));
outcome("typed off undefined:", () => worker.off("unknown", undefined as any));
outcome("alias add object:", () => worker.addListener("unknown", {} as any));
outcome("alias remove null:", () => worker.removeListener("unknown", null as any));

const on = process.argv[2] || "on";
const once = process.argv[3] || "once";
const off = process.argv[4] || "off";
outcome("dynamic on null:", () => (worker as any)[on]("unknown", null));
outcome("dynamic once number:", () => (worker as any)[once]("unknown", 1));
outcome("dynamic off undefined:", () => (worker as any)[off]("unknown", undefined));

const { port1, port2 } = new MessageChannel();
outcome("port symbol:", () => port1.on("unknown", Symbol("x") as any));
port1.close();
port2.close();

function removed() {
  events.push("removed");
}
worker.on("message", removed);
worker.removeListener("message", removed);
worker.addListener("message", (value) => events.push(`alias:${value}`));
worker.once("message", (value) => events.push(`once:${value}`));
worker.on("exit", (code) => {
  console.log("events:", events.join(","));
  console.log("exit:", code);
});
