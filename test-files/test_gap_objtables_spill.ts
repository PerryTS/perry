const owner: any = {};
for (let i = 0; i < 80; i++) owner['p' + i] = { value: i };
owner.p70 = undefined;
for (let i = 0; i < 50000; i++) { const x = { i, text: 'pressure' + i }; if (x.i < 0) console.log(x); }
console.log(owner.p79.value, Object.hasOwn(owner, 'p70'), owner.p70 === undefined);
const fresh: any = {};
console.log(fresh.p79 === undefined);
delete owner.p78;
console.log(Object.hasOwn(owner, 'p78'), Object.keys(owner).length);
