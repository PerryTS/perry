import { Worker } from "node:worker_threads";

class Meter {
  value = 0;
  writes = 0;
  get points(): number { return this.value; }
  set points(next: number) { this.value = next; this.writes++; }
}

const meter: any = new Meter();
function write(target: any, next: number): void { target.points = next; }
function run(start: number): void {
  for (let i = start; i < start + 1200; i++) write(meter, i % 17);
}
function collect(): void {
  let churn: any[] = [];
  for (let i = 0; i < 20000; i++) churn.push({ i });
  (globalThis as any).gc();
  churn = [];
}
function report(label: string): void {
  console.log(label, meter.points, meter.writes);
}

run(0);
report("before");
if (process.env.A2_START_WORKER !== "1") {
  collect();
  run(1200);
  report("after");
  run(2400);
  report("after-again");
} else {
  process.chdir("tests/fixtures/one_shape_setter_site");
  const worker = new Worker("./worker.cjs");
  worker.on("message", (message: any) => {
    if (message === "ready") {
      collect();
      run(1200);
      report("after");
      run(2400);
      report("after-again");
      worker.postMessage("go");
    } else {
      console.log("worker", message);
      worker.terminate();
    }
  });
  worker.on("exit", (code: number) => console.log("worker-exit", code));
}
