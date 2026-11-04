// #13953: a WASI 0.2 component has no threads either. A piped stdin is read
// by the event loop polling fd 0 (no reader thread), then 'end' at end of file.
let text = "";
process.stdin.on("data", (chunk) => {
  text += chunk.toString();
});
process.stdin.on("end", () => {
  console.log("end", JSON.stringify(text));
});
