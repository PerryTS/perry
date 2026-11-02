import { register, imports } from "./generated/guest";
import { collect } from "perry/gc";

register({
  "": {
    guarded() {
      collect();
      // The synchronous entry stages this import for the private actor.
      imports[""].empty();
      collect();
      return 77;
    },
  },
});
