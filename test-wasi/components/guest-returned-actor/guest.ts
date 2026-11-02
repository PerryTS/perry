import { register, imports } from "./generated/guest";
import { collect } from "perry/gc";

register({
  guarded() {
    collect();
    // Root-world bindings are flat; named interfaces use an interface key.
    imports.empty();
    collect();
    return 77;
  },
});
