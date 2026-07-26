import domain from "node:domain";

const intercepted = domain.create().intercept(() => {});
console.log(intercepted.name, intercepted.length);
console.log(Object.prototype.hasOwnProperty.call(intercepted, "domain"));
