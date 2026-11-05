import dgram from "node:dgram";
import * as ns from "node:dgram";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const bare = require("dgram");
const prefixed = require("node:dgram");

console.log("default identities:", ns.default === dgram, dgram === bare, bare === prefixed);
console.log("callable identities:", ns.Socket === dgram.Socket, ns.createSocket === dgram.createSocket,
  bare.Socket === prefixed.Socket, bare.createSocket === prefixed.createSocket);
console.log("default own:", Object.hasOwn(dgram, "default"), Object.hasOwn(bare, "default"), Object.hasOwn(prefixed, "default"));
console.log("default in:", "default" in dgram, "default" in bare, "default" in prefixed);
console.log("default descriptor absent:", Object.getOwnPropertyDescriptor(dgram, "default") === undefined,
  Object.getOwnPropertyDescriptor(bare, "default") === undefined,
  Object.getOwnPropertyDescriptor(prefixed, "default") === undefined);
console.log("default spread own:", Object.hasOwn({ ...dgram }, "default"),
  Object.hasOwn({ ...bare }, "default"), Object.hasOwn({ ...prefixed }, "default"));
