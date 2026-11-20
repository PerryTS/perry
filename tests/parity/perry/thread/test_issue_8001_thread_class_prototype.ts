// #8001: class prototype registries used to be process-global maps keyed by a
// codegen class id, even though each value was a raw pointer into one agent's
// realm-local arena. The first agent to materialize `RealmBox.prototype` then
// supplied that same object to every later agent.
//
// Warm the main realm first so this cannot pass merely because the spawned
// agent wins first touch. The worker checks both failure directions: it must not
// inherit the main realm's prototype mutation, and its own mutation must be
// visible to its instances without changing the main realm afterward.
//
// perry-only (`perry/thread` has no Node equivalent), so the stored expected
// output is the parity gate.
import { spawn } from "perry/thread";

class RealmBox {
  value: number;

  constructor(value: number) {
    this.value = value;
  }
}

type MutablePrototype = RealmBox & { owner?: string };

const mainPrototype = RealmBox.prototype as MutablePrototype;
mainPrototype.owner = "main";
console.log("main:", String((new RealmBox(1) as MutablePrototype).owner));

function agentProbe(): string {
  const before = new RealmBox(2) as MutablePrototype;
  const leaked = String(before.owner);

  const agentPrototype = RealmBox.prototype as MutablePrototype;
  agentPrototype.owner = "agent";
  const after = new RealmBox(3) as MutablePrototype;
  const ownsPrototype = Object.getPrototypeOf(after) === RealmBox.prototype;

  return leaked + "/" + String(after.owner) + "/" + String(ownsPrototype);
}

const expected = "undefined/agent/true";
const observed = await spawn((): string => agentProbe());
console.log("spawn agent:", observed, "match:", observed === expected);
console.log("main after:", String((new RealmBox(4) as MutablePrototype).owner));
