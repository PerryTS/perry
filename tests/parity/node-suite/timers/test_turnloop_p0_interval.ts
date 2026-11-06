// turnloop P0 loop-statistics probe: a 10 ms interval that fires three times.
let count = 0;
const interval = setInterval(() => {
  count++;
  if (count === 3) {
    clearInterval(interval);
    console.log("interval fired", count);
  }
}, 10);
