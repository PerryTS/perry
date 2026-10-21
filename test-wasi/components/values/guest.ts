import { collect } from "perry/gc";
import { imports, register, type Exports } from "./generated/guest";

const api = imports["perry:all-values/values@1.0.0"];
const exports: Exports = {
  "perry:all-values/values@1.0.0": {
    echo(input) {
      collect();
      const result = api.echo(input);
      collect();
      return result;
    },
    notify(input) { collect(); api.notify(input); },
    scalar() { const result = api.scalar(); collect(); return result; }
  }
};
register(exports);
