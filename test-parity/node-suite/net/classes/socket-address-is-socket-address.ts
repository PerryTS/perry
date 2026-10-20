import { SocketAddress } from "node:net";
import * as net from "node:net";

// Public registration and own-payload admission. Inherited/proxy branding is
// a separate Node compatibility gap, not an alternative expected result here.
const address = new SocketAddress();
const parsed = SocketAddress.parse("127.0.0.1:80");
const predicate = SocketAddress.isSocketAddress;
const Alias = SocketAddress;
const detached = { check: predicate };

function observe(label: string, run: () => any) {
  try {
    console.log(label, run());
  } catch (error: any) {
    console.log(label, error.name, error.code);
  }
}

console.log("metadata:", typeof predicate, predicate.name, predicate.length);
observe("direct:", () => SocketAddress.isSocketAddress(address));
observe("alias:", () => Alias.isSocketAddress(address));
observe("bracket:", () => SocketAddress["isSocketAddress"](address));
observe("namespace:", () => net.SocketAddress.isSocketAddress(address));
observe("extracted:", () => predicate(address));
observe("detached:", () => detached.check(address));
observe("null receiver:", () => predicate.call(null, address));
observe("foreign receiver:", () => predicate.call({}, address));
observe("omitted:", () => SocketAddress.isSocketAddress());
observe("extracted omitted:", () => predicate());
observe("constructable:", () => {
  new (predicate as any)();
  return true;
});

for (const [label, value] of [
  ["instance", address],
  ["parsed", parsed],
  ["fraction", 1.5],
  ["negative fraction", -1.5],
  ["subnormal", Number.MIN_VALUE],
  ["negative subnormal", -Number.MIN_VALUE],
  ["NaN", NaN],
  ["infinity", Infinity],
  ["negative infinity", -Infinity],
  ["zero", 0],
  ["negative zero", -0],
  ["null", null],
  ["undefined", undefined],
  ["string", "127.0.0.1"],
  ["true", true],
  ["false", false],
  ["plain", {}],
  ["prototype forged", Object.create(SocketAddress.prototype)],
  ["array", []],
  ["buffer", Buffer.from([1])],
  ["function", () => {}],
] as any[]) {
  observe("direct " + label + ":", () => SocketAddress.isSocketAddress(value));
  observe("value " + label + ":", () => predicate(value));
}
