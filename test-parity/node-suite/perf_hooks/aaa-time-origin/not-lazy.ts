const start = Date.now();

setTimeout(() => {
  const elapsed = Date.now() - start;
  const originAge = Date.now() - performance.timeOrigin;

  console.log("elapsed observed:", elapsed >= 15);
  console.log("origin includes wait:", originAge + 5 >= elapsed);
  console.log("now includes wait:", performance.now() + 5 >= elapsed);
}, 25);
