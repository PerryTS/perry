// Issue #2398: console.log / util.inspect must render a Date as Node's ISO
// string, while its numeric timestamp (getTime() / valueOf() / a literal)
// keeps printing as a plain number. This works because #2089/#2380 made a
// Date a distinguishable heap reference — guarding against a regression of
// the kind PR #2397 (now obsolete) was reverting.
import util from "node:util";

// Direct constructor argument.
console.log(new Date(0));

// Inferred Date local.
const d = new Date(1700000000000);
console.log(d);

// Explicitly-annotated Date local.
const annotated: Date = new Date(0);
console.log(annotated);

// The numeric timestamp must still print as a number — not ISO.
console.log(d.getTime());
console.log(+d);
console.log(1700000000000);

// Mixed multi-arg: string + Date + string.
console.log("at", new Date(0), "done");

// Invalid Date.
console.log(new Date(NaN));

// Date arithmetic is unaffected.
console.log(d.getTime() - 1700000000000);

// Nested in an object/array: Date renders ISO, the timestamp stays numeric.
console.log({ when: new Date(0), ts: d.getTime() });
console.log([new Date(0), new Date(1700000000000)]);

// util.inspect returns the ISO string.
console.log(util.inspect(new Date(0)));
console.log(util.inspect({ when: new Date(0) }));
