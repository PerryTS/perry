import domain from "node:domain";

const d = domain.create();
const callback = () => console.log("callback");
const intercepted = d.intercept(callback);
d.on("error", (error: any) => {
  console.log(error.message, error.domain === d);
  console.log(
    error.domainThrown,
    error.domainBound === callback,
    error.domainEmitter,
  );
});
console.log(String(intercepted(new Error("intercepted"), "ignored")));
