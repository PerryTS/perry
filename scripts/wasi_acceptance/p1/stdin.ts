// A piped stdin is read without helper threads: the event loop parks on fd 0
// and delivers the data, then 'end' at end of file.
let text = "";
process.stdin.on("data", (chunk) => {
  text += chunk.toString();
});
process.stdin.on("end", () => {
  console.log("end", JSON.stringify(text));
});
