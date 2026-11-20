// Issue #9148: function-object expandos follow OrdinaryOwnPropertyKeys.
const f: any = function () {};

f.tag = 1;
f.other = 2;
console.log(JSON.stringify(Object.keys(f)));

// Integer-index keys precede strings even when added later. Other strings
// retain creation order rather than HashMap/alphabetical order.
f["10"] = "ten";
f["2"] = "two";
f.alpha = 3;
console.log(JSON.stringify(Object.keys(f)));
console.log(JSON.stringify(Object.values(f)));
console.log(JSON.stringify(Object.entries(f)));

const forIn: string[] = [];
for (const key in f) forIn.push(key);
console.log(JSON.stringify(forIn));
console.log(JSON.stringify(Object.keys({ ...f })));

// Updating does not move a key; deleting and re-adding does.
f.tag = 4;
console.log(JSON.stringify(Object.keys(f)));
delete f.other;
f.other = 5;
console.log(JSON.stringify(Object.keys(f)));

// getOwnPropertyNames includes intrinsic function keys, but its expando
// subsequence obeys the same integer/string ordering.
const ownNames = Object.getOwnPropertyNames(f).filter((key: string) =>
  key === "2" || key === "10" || key === "tag" || key === "alpha" || key === "other"
);
console.log(JSON.stringify(ownNames));
