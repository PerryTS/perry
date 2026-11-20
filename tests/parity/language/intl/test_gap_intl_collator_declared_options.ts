// Intl.Collator reads its options (ToString of string options runs the
// program toString), supportedLocalesOf returns fresh strings, and compare
// is the prototype getter returning the bound compare function.
const reads: string[] = [];
const opts = {
  get usage() { reads.push("usage"); return { toString() { reads.push("usage.toString"); return "sort"; } }; },
  get numeric() { reads.push("numeric"); return true; },
  get sensitivity() { reads.push("sensitivity"); return "base"; },
};
const l = Intl.Collator.supportedLocalesOf("en");
console.log(Array.isArray(l), l.length, typeof l[0]);
const c = new Intl.Collator(l.length ? l[0] : "en", opts as any);
console.log(reads.join(","));
const cmp = c.compare;
console.log(cmp === c.compare, cmp.length, cmp.name === "", Object.getOwnPropertyNames(c).length);
console.log(cmp("a", "b"), cmp("b", "a"), cmp("a", "A"), cmp("2", "10"));
console.log(["b", "a", "C"].sort(c.compare).join(","));
console.log(c.resolvedOptions().numeric, c.resolvedOptions().sensitivity, c.resolvedOptions().usage);
