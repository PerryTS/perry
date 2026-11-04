// #13953: on WASI 0.2 as on preview 1, a loop that never parks (a self-rearming
// setImmediate) still receives piped stdin: the event pump polls fd 0.
let text = "";
let spins = 0;
let ended = false;
const started = Date.now();
function spin() {
  spins++;
  if (ended) return;
  if (Date.now() - started > 20000) {
    console.log("timed out");
    return;
  }
  setImmediate(spin);
}
process.stdin.on("data", (chunk) => {
  text += chunk.toString();
});
process.stdin.on("end", () => {
  ended = true;
  console.log("end", JSON.stringify(text), spins > 1);
});
setImmediate(spin);
