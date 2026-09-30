// Eleven receiver shapes at one read site, sharing one absent null-prototype
// terminal. The last shape rotates the ten-shape site and must remain correct.
const terminal: any = Object.create(null);
const receivers: any[] = [];
for (let n = 0; n < 11; n++) {
  const o: any = Object.create(terminal);
  for (let k = 0; k < n; k++) o["field" + k] = k;
  receivers.push(o);
}

function read(o: any): number {
  const value = o.missing;
  return value === undefined ? 0 : value;
}

let sum = 0;
for (let i = 0; i < 550; i++) sum += read(receivers[i % receivers.length]);
console.log("all-absent", sum);

// The terminal is young when the site primes. A forced collection must keep
// and rewrite the site's rooted holder; a later own-key shadow cannot reuse
// the same receiver ShapeId and must win over the absent entry.
let churn: any[] = [];
for (let i = 0; i < 20000; i++) churn.push({ i });
(globalThis as any).gc();
churn = [];
receivers[3].missing = 5;
sum = 0;
for (let i = 0; i < 550; i++) sum += read(receivers[i % receivers.length]);
console.log("own-shadow", sum);

// A new terminal shape invalidates every stored receiver shape. Reassigning
// the terminal value without changing its shape must be observed as well.
terminal.missing = 7;
sum = 0;
for (let i = 0; i < 550; i++) sum += read(receivers[i % receivers.length]);
console.log("terminal-add", sum);
terminal.missing = 9;
sum = 0;
for (let i = 0; i < 550; i++) sum += read(receivers[i % receivers.length]);
console.log("terminal-value", sum);
delete terminal.missing;
sum = 0;
for (let i = 0; i < 550; i++) sum += read(receivers[i % receivers.length]);
console.log("terminal-delete", sum);

// Two receivers with the same own key list can have different prototype
// identities. Their absent facts must not be shared through the site.
const otherTerminal: any = Object.create(null);
otherTerminal.missing = 13;
const otherReceiver: any = Object.create(otherTerminal);
otherReceiver.field0 = 0;
sum = 0;
for (let i = 0; i < 550; i++) sum += read(i % 2 ? otherReceiver : receivers[1]);
console.log("different-terminal", sum);
