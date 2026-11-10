console.log("start");
Promise.resolve().then(() => console.log("microtask"));
setTimeout(() => console.log("timer"), 20);
