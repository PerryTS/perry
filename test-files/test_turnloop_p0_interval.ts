let count = 0;
const interval = setInterval(() => {
  count++;
  if (count === 3) {
    clearInterval(interval);
    console.log("interval", count);
  }
}, 10);
