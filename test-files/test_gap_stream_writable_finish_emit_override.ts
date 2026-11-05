// A Writable raises 'finish' through its own `emit` method, so a subclass that
// overrides `emit` sees the event and decides whether it goes out. Multipart
// parsers (Dicer in `@fastify/busboy`) hold 'finish' back until their parts
// have ended and emit it later themselves; with 'finish' emitted behind the
// override's back the early event reached the listeners and the late one
// followed it.
import { Writable } from "node:stream";

const log: string[] = [];

class HoldFinish extends Writable {
  released = false;
  _write(_chunk: any, _enc: string, cb: () => void): void {
    cb();
  }
  emit(event: string, ...args: any[]): boolean {
    if (event === "finish" && !this.released) {
      log.push("override saw finish, held back");
      setTimeout(() => {
        this.released = true;
        log.push("override releases finish");
        this.emit("finish");
      }, 5);
      return false;
    }
    return super.emit(event, ...args);
  }
}

class CountEmits extends Writable {
  _write(_chunk: any, _enc: string, cb: () => void): void {
    cb();
  }
  emit(event: string, ...args: any[]): boolean {
    if (event === "finish") log.push("pass-through override saw finish");
    return super.emit(event, ...args);
  }
}

function run(w: Writable, name: string): Promise<void> {
  return new Promise((resolve) => {
    w.on("finish", () => {
      log.push(`${name} finish listener`);
      resolve();
    });
    w.write("a");
    w.end();
  });
}

(async () => {
  await run(new HoldFinish(), "held");
  await run(new CountEmits(), "counted");
  console.log(log.join("\n"));
})();
