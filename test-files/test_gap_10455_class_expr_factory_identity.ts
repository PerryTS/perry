// #10455: a class expression returned from a function had no per-evaluation
// identity when the function lived in a NON-ENTRY module — every call
// returned the SAME class object, re-parented to the most recently passed
// `Base`. A mixin/factory declared and called in the entry module (or
// specialized per call site by `specialize_captured_class_factories`) always
// worked; the same shape reached through an ordinary cross-module call did
// not, because the per-module specialization pass never sees callers outside
// its own module. redis's `commander.js` `attachConfig` (`Class = class
// extends BaseClass {}`) hits exactly this from `RedisClient.factory` and
// `Client.prototype.Multi = MultiCommand.extend(config)`.
import { withCommands, withCommandsB } from "./gap_10455_class_expr_factory_identity_helper.ts";

class A {
  constructor() {
    console.log("  A constructor");
  }
  hello() {
    return "A.hello";
  }
}
class B {
  constructor() {
    console.log("  B constructor");
  }
  world() {
    return "B.world";
  }
}

function localWithCommands(Base: any) {
  return class extends Base {};
}

// ── entry-module factory: two calls, two evaluations ──
const LocalA = localWithCommands(A);
const LocalB = localWithCommands(B);
const la = new LocalA();
console.log(
  "entry module:   LocalA !== LocalB:",
  LocalA !== LocalB,
  "| instanceof A:",
  la instanceof A,
  "| typeof hello:",
  typeof (la as any).hello,
);

// ── non-entry-module factory: two calls, two evaluations ──
const ClientA = withCommands(A);
const ClientB = withCommands(B);
const ca = new ClientA();
const cb = new ClientB();
console.log(
  "non-entry module: ClientA !== ClientB:",
  ClientA !== ClientB,
  "| instanceof A:",
  ca instanceof A,
  "| typeof hello:",
  typeof (ca as any).hello,
);
console.log(
  "non-entry module: cb instanceof B:",
  cb instanceof B,
  "| cb instanceof A:",
  cb instanceof A,
  "| typeof world:",
  typeof (cb as any).world,
);
// ── three sequential calls at the SAME call site (loop) also stay distinct ──
const made: any[] = [];
for (let i = 0; i < 3; i++) {
  made.push(withCommandsB(i % 2 === 0 ? A : B));
}
console.log(
  "loop calls pairwise distinct:",
  made[0] !== made[1] && made[1] !== made[2] && made[0] !== made[2],
);
console.log(
  "loop instances match their own parent:",
  new made[0]() instanceof A,
  new made[1]() instanceof B,
  new made[2]() instanceof A,
);
