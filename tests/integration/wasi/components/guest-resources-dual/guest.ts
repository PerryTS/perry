import { collect } from "perry/gc";
import { imports, register, resources } from "./generated/guest";
const local = resources["perry:guest-resources/foreign@1.0.0"].token;
const foreign = imports["perry:guest-resources/foreign@1.0.0"];
function check(ok: boolean, message: string): void { if (!ok) throw new Error(message); }
register({
  "perry:guest-resources/foreign@1.0.0": {
    "[constructor]token"(value) {
      const result = local.create({ value }, (payload) => {
        collect();
        imports["perry:guest-resources/observe@1.0.0"].dropped((payload as { value: number }).value);
      });
      const imported = foreign["[constructor]token"](value + 1);
      let rejected = false;
      // Deliberate dynamic misuse checks that nominal identity survives beyond
      // the generated TypeScript declarations.
      try { foreign["[method]token.read"](result.borrow() as any); } catch (_) { rejected = true; }
      check(rejected, "guest owner substituted for imported resource");
      rejected = false;
      try { local.value(imported.borrow() as any); } catch (_) { rejected = true; }
      check(rejected, "imported owner substituted for guest resource");
      collect();
      check(foreign["[method]token.read"](imported.borrow()) === value + 1, "import handle corrupted");
      imported.dispose();
      return result;
    },
    "[method]token.read"(value) { collect(); return (local.value(value) as { value: number }).value; },
    consume(value) {
      const result = (local.value(value) as { value: number }).value;
      value.dispose();
      return result;
    },
    "consume-pair"(pair) {
      const result = (local.value(pair.first) as { value: number }).value + (local.value(pair.second) as { value: number }).value;
      pair.first.dispose(); pair.second.dispose();
      return result;
    }
  }
});
