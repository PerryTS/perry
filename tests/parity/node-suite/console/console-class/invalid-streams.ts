import { Console } from "node:console";
import { Writable } from "node:stream";

const sink = new Writable({
  write(_chunk: any, _enc: string, cb: (err?: Error | null) => void) {
    cb();
  },
});

function probe(label: string, construct: () => void): void {
  try {
    construct();
    console.log(label, "OK");
  } catch (err: any) {
    console.log(label, "THROW", err?.name, err?.code || "no-code", err?.message);
  }
}

probe("no args", () => { new (Console as any)(); });
probe("undefined stdout", () => { new (Console as any)(undefined); });
probe("object no write", () => { new (Console as any)({}); });
probe("options stdout missing", () => { new (Console as any)({}); });
probe("options stdout no write", () => { new Console({ stdout: {} as any }); });
probe("options stdout write nonfn", () => { new Console({ stdout: { write: 1 } as any }); });
probe("options stderr no write", () => { new Console({ stdout: sink, stderr: {} as any }); });
probe("options stderr write nonfn", () => { new Console({ stdout: sink, stderr: { write: 1 } as any }); });
probe("single writable", () => { new Console(sink); });
probe("options stdout only", () => { new Console({ stdout: sink }); });
