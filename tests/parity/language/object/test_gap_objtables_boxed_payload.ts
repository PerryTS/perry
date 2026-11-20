const wrappers: any[] = [new Number(42), new String('private-payload'), new Boolean(true), Object(17n), Object(Symbol.for('boxed'))];
for (const w of wrappers) w.extra = 'own';
for (let i = 0; i < 6; i++) {
    const garbage: any[] = [];
    for (let j = 0; j < 4000; j++) garbage.push({ j, text: 'boxed-' + j });
}
for (const w of wrappers) console.log(String(w.valueOf()), Object.keys(w).join(','));
console.log(JSON.stringify(wrappers.slice(0, 3)));
const ordinary: any = { valueOf() { return 99; } };
console.log(ordinary.valueOf(), Object.keys(ordinary).join(','));
