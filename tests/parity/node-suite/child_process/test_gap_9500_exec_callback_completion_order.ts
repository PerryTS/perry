// #9500 (part 2): `cp.exec` / `cp.execFile` callbacks fire in COMPLETION order,
// not submission order — a child that finishes first calls back first
// regardless of which API launched it or which call came first. (When both
// children finish inside the same loop turn, node's order is a libuv
// batch-delivery artefact, not a rule — the issue's "exec→execFile vs
// execFile→exec" for two instant `echo`s — so only the order with a real
// completion gap is pinned here.)
import * as cp from "node:child_process";

function race(label: string, first: () => Promise<string>, second: () => Promise<string>) {
  const order: string[] = [];
  const tag = (name: string) => (p: Promise<string>) => p.then((out) => { order.push(`${name}:${out}`); });
  return Promise.all([tag("A")(first()), tag("B")(second())]).then(() => {
    console.log(label, "→", order.join(" "));
  });
}
const exec = (cmd: string) => new Promise<string>((res) => cp.exec(cmd, (_e, out) => res(String(out).trim())));
const execFile = (file: string, args: string[]) => new Promise<string>((res) => cp.execFile(file, args, (_e, out) => res(String(out).trim())));

// exec submitted first but slow; execFile submitted second and instant.
race("slow exec, instant execFile", () => exec("sleep 0.3; echo slow"), () => execFile("/bin/echo", ["fast"]))
  // execFile submitted first but slow (via sh); exec second and instant.
  .then(() => race("slow execFile, instant exec", () => execFile("/bin/sh", ["-c", "sleep 0.3; echo slow"]), () => exec("echo fast")))
  // three children with staggered durations, submitted longest-first.
  .then(() => {
    const order: string[] = [];
    return Promise.all([
      exec("sleep 0.45; echo c").then((o) => { order.push(o); }),
      execFile("/bin/sh", ["-c", "sleep 0.3; echo b"]).then((o) => { order.push(o); }),
      exec("sleep 0.15; echo a").then((o) => { order.push(o); }),
    ]).then(() => console.log("staggered → " + order.join(" ")));
  });
