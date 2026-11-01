console.log("start");
Promise.resolve().then(() => console.log("microtask"));
setTimeout(() => {
  console.log("timer", Date.now() > 0);
}, 5);
