import { channel, Channel, boundedChannel, BoundedChannel } from "node:diagnostics_channel";

const ordinary = channel("dc-instanceof-precedence");
const bounded = boundedChannel("dc-instanceof-precedence");
const foreign = {};
const calls: string[] = [];

function check(name: string, ctor: any, instance: any, other: any) {
  console.log(name, "brand:", instance instanceof ctor);
  console.log(name, "noninstances:", foreign instanceof ctor, other instanceof ctor, 1 instanceof ctor);
  const original = Object.getOwnPropertyDescriptor(ctor, Symbol.hasInstance);
  Object.defineProperty(ctor, Symbol.hasInstance, {
    configurable: true,
    value(value: any) {
      calls.push(`${name}:${value === instance ? "instance" : "foreign"}`);
      return value === foreign;
    },
  });
  console.log(name, "own hook:", instance instanceof ctor, foreign instanceof ctor);
  if (original) Object.defineProperty(ctor, Symbol.hasInstance, original);
  else delete ctor[Symbol.hasInstance];
  console.log(name, "restored:", instance instanceof ctor);
}

check("Channel", Channel, ordinary, bounded);
check("BoundedChannel", BoundedChannel, bounded, ordinary);
console.log("hook calls:", calls.join("|"));
