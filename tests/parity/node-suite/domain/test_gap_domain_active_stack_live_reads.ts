// domain.active / domain._stack must be re-read after exit(), not frozen at the first read.
import domain from "node:domain";

const d = domain.create();
console.log("before enter — active:", String(domain.active), "stack len:", domain._stack.length);

d.enter();
console.log(
  "while entered — active === d:",
  domain.active === d,
  "stack len:",
  domain._stack.length
);

const observedWhileEntered = domain.active;

d.exit();
console.log(
  "after exit — active:",
  String(domain.active),
  "stack len:",
  domain._stack.length,
  "observed-while-entered was live:",
  observedWhileEntered === d
);
