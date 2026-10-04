// #11791: a structured clone (structuredClone, postMessage) copies own
// enumerable PROPERTIES only. Private fields are entries of the shape and a
// brand is a shape fact; neither is a property, so the copy has neither, and
// the copy is a plain object.
import { MessageChannel, receiveMessageOnPort } from "node:worker_threads";

class Secret {
  #secret = 7;
  #label = "hidden";
  pub = 1;
  #m() {
    return 1;
  }
  static fields(o: any) {
    return #secret in o;
  }
  static brand(o: any) {
    return #m in o;
  }
  static read(o: any) {
    try {
      return o.#secret;
    } catch (e: any) {
      return e.constructor.name + ": " + e.message;
    }
  }
}

class Derived extends Secret {
  #more = [1, 2];
  extra = "x";
  static more(o: any) {
    return #more in o;
  }
}

function describe(tag: string, o: any) {
  console.log(
    tag,
    JSON.stringify(o),
    Object.keys(o).join(),
    Object.getOwnPropertyNames(o).join(),
    Object.getPrototypeOf(o) === Object.prototype,
    Secret.fields(o),
    Secret.brand(o),
    Derived.more(o),
    Secret.read(o),
  );
}

const s = new Secret();
const d = new Derived();
describe("original", s);
describe("derived", d);
describe("clone", structuredClone(s));
describe("clone-derived", structuredClone(d));
describe("clone-nested", structuredClone({ inner: d, list: [s] }).inner);

const sync = new MessageChannel();
sync.port1.postMessage(d);
const got = receiveMessageOnPort(sync.port2);
describe("port-sync", got && got.message);
sync.port1.close();
sync.port2.close();

(async () => {
  await new Promise<void>((resolve) => {
    const ch = new MessageChannel();
    ch.port2.on("message", (value: any) => {
      describe("port-async", value);
      ch.port1.close();
      ch.port2.close();
      resolve();
    });
    ch.port1.postMessage(s);
  });
  // The originals still carry everything.
  describe("original-after", s);
})();
