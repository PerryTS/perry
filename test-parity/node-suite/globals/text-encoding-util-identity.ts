import util, {
  TextDecoder as NamedTextDecoder,
  TextEncoder as NamedTextEncoder,
} from "node:util";
import * as utilNs from "node:util";

console.log(
  "namespace encoder identity:",
  globalThis.TextEncoder === utilNs.TextEncoder,
);
console.log(
  "namespace decoder identity:",
  globalThis.TextDecoder === utilNs.TextDecoder,
);
console.log(
  "default encoder identity:",
  globalThis.TextEncoder === (util as any).TextEncoder,
);
console.log(
  "default decoder identity:",
  globalThis.TextDecoder === (util as any).TextDecoder,
);
console.log(
  "named encoder identity:",
  globalThis.TextEncoder === NamedTextEncoder,
);
console.log(
  "named decoder identity:",
  globalThis.TextDecoder === NamedTextDecoder,
);
console.log(
  "roundtrip:",
  new TextDecoder().decode(new TextEncoder().encode("hé")),
);
