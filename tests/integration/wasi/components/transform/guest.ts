import { collect } from "perry/gc";
import { imports, register, type Exports } from "./generated/guest";

const exports: Exports = {
  "perry:transform/transform@1.0.0": {
    transform(input) {
      imports["perry:transform/logging@1.0.0"].log(input.text);
      collect();
      if (input.text.length === 0) {
        return { tag: "err", value: { tag: "invalid-input", value: "text must not be empty" } };
      }
      const bytes = new Uint8Array(input.bytes.length);
      for (let i = 0; i < bytes.length; i++) bytes[i] = input.bytes[bytes.length - i - 1];
      return { tag: "ok", value: { text: input.text + " / Perry", bytes } };
    }
  }
};

register(exports);
