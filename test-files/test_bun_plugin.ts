// #10100: setup must run during registration, without invoking loader hooks.
import { plugin } from "bun";
let installed = false;
plugin({ name: "startup", setup(build) {
  build.onLoad({ filter: /a/ }, () => { throw new Error("loader ran"); });
  installed = true;
} });
console.log(installed);
Bun.plugin((build) => { build.onResolve({ filter: /a/ }, () => ({})); });
console.log(Bun.plugin.clearAll() === undefined);
console.log(typeof Bun);
