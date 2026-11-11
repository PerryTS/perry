// #6316: a subclass method that OVERRIDES a native base-class method must run,
// and `super.<method>()` from inside it must reach the native base.
//
// perry models EventEmitter and the node:stream classes by stamping the base's
// method surface onto the INSTANCE as ordinary own properties. Own properties
// legitimately shadow class methods, so perry's own-property-override probe
// (#620) picked the native closure over the user's override: inheritance ran
// backwards and the override never executed — silently, with no error. The
// `super.<m>()` path into such a base was independently dead: a native base is
// not a perry class, so the subclass has no registered parent edge to resolve
// against and `super.emit(...)` returned undefined.

import { EventEmitter } from "node:events";
import { Readable, Writable } from "node:stream";

// ── EventEmitter: override + super, the issue's exact repro ──
class Bus extends EventEmitter {
  emit(ev: string, ...args: any[]): boolean {
    console.log("Bus.emit override ran:", ev);
    return super.emit(ev, ...args);
  }
}
const bus = new Bus();
bus.on("ping", (x: number) => console.log("bus listener got:", x));
console.log("bus emit returned:", bus.emit("ping", 42));

// ── several overrides at once, each super-calling the base ──
class Chatty extends EventEmitter {
  events: string[] = [];
  on(ev: string, fn: any): this {
    console.log("Chatty.on override ran:", ev);
    this.events.push(ev);
    return super.on(ev, fn);
  }
  emit(ev: string, ...args: any[]): boolean {
    console.log("Chatty.emit override ran:", ev);
    return super.emit(ev, ...args);
  }
}
const chatty = new Chatty();
chatty.on("a", (v: string) => console.log("chatty listener got:", v));
console.log("chatty emit returned:", chatty.emit("a", "hello"));
console.log("chatty registered:", chatty.events.join(","));

// ── a NON-overridden method on an overriding subclass still reaches the base ──
console.log("chatty listenerCount:", chatty.listenerCount("a"));
chatty.removeAllListeners("a");
console.log("chatty after removeAll:", chatty.listenerCount("a"));

// ── no-override control: plain subclass keeps working ──
class Plain extends EventEmitter {}
const plain = new Plain();
plain.on("go", (v: number) => console.log("plain listener got:", v));
console.log("plain emit returned:", plain.emit("go", 7));

// ── an explicit constructor + state: the `super()` install path ──
// (`class X extends Base {}` where Base is itself the EventEmitter subclass is
// NOT covered here — an INDIRECT native base never runs the subclass init at
// all, override or not. That is a separate pre-existing gap, not this issue.)
class Counter extends EventEmitter {
  seen: number;
  constructor(start: number) {
    super();
    this.seen = start;
  }
  emit(ev: string, ...args: any[]): boolean {
    this.seen++;
    console.log("Counter.emit override ran:", ev, "seen:", this.seen);
    return super.emit(ev, ...args);
  }
}
const counter = new Counter(0);
counter.on("tick", () => console.log("counter listener fired"));
console.log("counter emit returned:", counter.emit("tick"));
console.log("counter emit returned:", counter.emit("tick"));

// ── the displaced base method must not leak as an own key ──
const keys = Object.keys(bus);
console.log("bus own 'emit' key present:", keys.includes("emit"));

// ── node:stream — same install-on-instance mechanism as EventEmitter ──
class CountingReadable extends Readable {
  pushes = 0;
  push(chunk: any, enc?: any): boolean {
    this.pushes++;
    console.log("CountingReadable.push override ran");
    return super.push(chunk, enc);
  }
  _read(): void {
    this.push(null);
  }
}
const rs = new CountingReadable();
rs.on("end", () => {
  console.log("readable end, pushes:", rs.pushes);

  // ── Writable: override `write`, forward to the base ──
  class LoggingWritable extends Writable {
    written: string[] = [];
    write(chunk: any, enc?: any, cb?: any): boolean {
      console.log("LoggingWritable.write override ran");
      this.written.push(String(chunk));
      return super.write(chunk, enc, cb);
    }
    _write(_chunk: any, _enc: any, cb: any): void {
      cb();
    }
  }
  const ws = new LoggingWritable();
  const ok = ws.write("payload");
  console.log("writable write returned:", ok);
  console.log("writable captured:", ws.written.join(","));
});
rs.resume();
