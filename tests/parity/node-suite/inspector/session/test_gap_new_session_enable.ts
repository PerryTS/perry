// `new` of the realm constructor node:inspector.Session, over a namespace
// import, a default import and a named import: each makes a session that
// connects, posts and calls back.
import * as ns from "node:inspector";
import def from "node:inspector";
import { Session } from "node:inspector";

let calls = 0;
function enable(label: string, make: () => InstanceType<typeof Session>, next: () => void) {
  const session = make();
  console.log(label, session instanceof Session, typeof session.post);
  session.connect();
  session.post("Profiler.enable", (err: unknown) => {
    calls++;
    console.log(label, "enable callback", err === null, calls);
    session.disconnect();
    next();
  });
}

enable("namespace", () => new ns.Session(), () =>
  enable("default", () => new def.Session(), () =>
    enable("named", () => new Session(), () => console.log("done", calls))));
