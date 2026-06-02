import * as dc from "node:diagnostics_channel";

const name = "dc-duplicate-subscribe";
let calls = 0;
function listener(data: any) {
  calls += data.step;
}
dc.subscribe(name, listener);
dc.subscribe(name, listener);
dc.channel(name).publish({ step: 1 });
console.log("after duplicate publish:", calls);
console.log("first unsubscribe:", dc.unsubscribe(name, listener));
dc.channel(name).publish({ step: 1 });
console.log("after one unsubscribe:", calls);
console.log("second unsubscribe:", dc.unsubscribe(name, listener));
console.log("has after second:", dc.hasSubscribers(name));
