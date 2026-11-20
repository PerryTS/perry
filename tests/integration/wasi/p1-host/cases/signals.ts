// The signal stream delivers (USR1 and an unknown byte are ignored), reaches
// end of stream at 200 ms, and the loop keeps its timer without spinning: the
// suite bounds the number of polls the guest made.
let ticks = 0;
process.on("SIGUSR2", () => console.log("usr2"));
process.on("SIGINT", () => console.log("int"));
const timer = setInterval(() => {
  if (++ticks === 8) {
    clearInterval(timer);
    console.log("done");
  }
}, 50);
