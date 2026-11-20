import { readFileSync, writeFileSync } from "node:fs";
import { register } from "./generated/guest";
register({
  "perry:capabilities/probe@1.0.0": {
    arguments() { return process.argv; },
    environment(name) {
      const value = process.env[name];
      return value === undefined ? { tag: "none" } : { tag: "some", value };
    },
    read(path) {
      try { return { tag: "ok", value: readFileSync(path, "utf8") }; }
      catch (error) { return { tag: "err", value: String(error) }; }
    },
    write(path, text) {
      try { writeFileSync(path, text); return { tag: "ok" }; }
      catch (error) { return { tag: "err", value: String(error) }; }
    }
  }
});
