"use strict";
import fsDefault from "node:fs";
import pathDefault from "node:path";
// Strict mode: console members and CommonJS exports are configurable, so delete
// succeeds (no TypeError); fs.constants is not, so deleting it throws.
const c: any = console;
let r: boolean | string;
try { r = delete c.log; } catch (e) { r = "threw " + (e as Error).constructor.name; }
process.stdout.write("delete " + r + " typeof " + typeof c.log + " own " + Object.prototype.hasOwnProperty.call(c, "log") + "\n");
try { console.log("hello"); process.stdout.write("no throw\n"); } catch (e) { process.stdout.write("threw " + (e as Error).constructor.name + "\n"); }
const fs: any = fsDefault;
let f: boolean | string;
try { f = delete fs.constants; } catch (e) { f = "threw " + (e as Error).constructor.name; }
process.stdout.write("fs.constants " + f + " " + typeof fs.constants + "\n");
const p: any = pathDefault;
process.stdout.write("path.join " + (delete p.join) + " " + typeof p.join + " " + ("join" in p) + "\n");
try { p.join("a", "b"); process.stdout.write("no throw\n"); } catch (e) { process.stdout.write("threw " + (e as Error).constructor.name + "\n"); }
