console.log("start");
const started = Date.now();
Promise.resolve().then(() => console.log("microtask"));
setTimeout(() => {
  console.log("timer", Date.now() - started >= 1);
}, 5);
