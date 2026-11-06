// process.stdin starts paused: an "end"/"close"/"error" listener must not start
// reading fd 0. Only data/readable listeners, resume(), pipe(), read() or async
// iteration do, and readableFlowing follows Node's null/true/false transitions.
const stdin = process.stdin;
const state = (label: string) =>
  console.log(label, "flowing", stdin.readableFlowing, "paused", stdin.isPaused());

state("initial");

stdin.once("end", () => {});
stdin.on("close", () => {});
stdin.on("error", () => {});
state("after end/close/error listeners");

stdin.resume();
state("after resume");

stdin.pause();
state("after pause");

stdin.resume();
state("after second resume");

stdin.pause();
state("final");
