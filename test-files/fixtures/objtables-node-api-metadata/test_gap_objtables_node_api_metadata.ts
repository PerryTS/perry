import addon from 'objtables-metadata-addon';
const owner = {answer: 42};
console.log(addon.attach(owner));
for (let i = 0; i < 40000; i++) { const a = {i, nested: [i, i + 1]}; if (i === 39999) console.log(a.i); }
console.log(addon.read(owner));
console.log(owner.answer);
console.log(Object.keys(owner).join(','));
console.log(addon.read({answer: 99}));
