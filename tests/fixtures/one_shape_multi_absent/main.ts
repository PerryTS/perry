// Eleven receiver shapes at one read site share Object.prototype as their
// absent terminal. Ten warm shapes must hit; the last rotates the site.
const terminal: any = Object.prototype;
const receivers: any[] = [];
for (let n = 0; n < 11; n++) {
  const o: any = {};
  for (let k = 0; k < n; k++) o["field" + k] = k;
  receivers.push(o);
}

function read(o: any): number {
  const value = o.a2MissingFacet;
  return value === undefined ? 0 : value;
}

let sum = 0;
for (let i = 0; i < 550; i++) sum += read(receivers[i % 10]);
sum += read(receivers[10]);
console.log("all-absent", sum);

// A forced collection must keep and rewrite the site's rooted terminal. A
// later own-key shadow changes the receiver ShapeId and must win.
let churn: any[] = [];
for (let i = 0; i < 20000; i++) churn.push({ i });
(globalThis as any).gc();
churn = [];
receivers[3].a2MissingFacet = 5;
sum = 0;
for (let i = 0; i < 550; i++) sum += read(receivers[i % 10]);
console.log("own-shadow", sum);

// A new terminal shape invalidates every stored absent proof. Reassigning
// its value without changing the shape must also be observed.
terminal.a2MissingFacet = 7;
sum = 0;
for (let i = 0; i < 550; i++) sum += read(receivers[i % 10]);
console.log("terminal-add", sum);
terminal.a2MissingFacet = 9;
sum = 0;
for (let i = 0; i < 550; i++) sum += read(receivers[i % 10]);
console.log("terminal-value", sum);
delete terminal.a2MissingFacet;
sum = 0;
for (let i = 0; i < 550; i++) sum += read(receivers[i % 10]);
console.log("terminal-delete", sum);

// Two receivers with the same own key list can have different prototype
// identities. Their absent facts must not be shared through the site.
const otherTerminal: any = Object.create(null);
otherTerminal.a2MissingFacet = 13;
const otherReceiver: any = Object.create(otherTerminal);
otherReceiver.field0 = 0;
sum = 0;
for (let i = 0; i < 550; i++) sum += read(i % 2 ? otherReceiver : receivers[1]);
console.log("different-terminal", sum);
