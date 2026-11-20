const prototype = { marker: 37 };
const owners: any[] = [[], new Map(), new Set(), new Date(0), /a/g,
  new Error("owned"), Promise.resolve(1), function owned() {}, new Uint8Array(2)];
for (const owner of owners) Object.setPrototypeOf(owner, prototype);
for (let i = 0; i < 40000; i++) {
  const temporary = { i, text: "prototype-allocation-" + i };
  if (temporary.i === -1) console.log(temporary.text);
}
for (const owner of owners) {
  const recorded = Object.getPrototypeOf(owner);
  console.log(recorded === prototype, recorded.marker);
}
const fresh: any[] = [];
console.log(Object.getPrototypeOf(fresh) === Array.prototype, fresh.marker);
Object.setPrototypeOf(owners[0], null);
console.log(Object.getPrototypeOf(owners[0]) === null, owners[0].marker);
