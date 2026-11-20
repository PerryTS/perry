import domain from "node:domain";

const d = domain.create();
d.on("error", (error: any) => {
  console.log(error.message, error.domain === d, error.domainThrown);
  console.log(error.domainEmitter, error.domainBound);
});
d.intercept(() => {
  throw new Error("callback");
})(null);
