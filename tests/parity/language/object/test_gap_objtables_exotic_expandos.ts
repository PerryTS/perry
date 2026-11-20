const owners: any[] = [new Date(0), new Map(), new Set(), Promise.resolve(1), new Error('own'), new Temporal.PlainDate(2026, 10, 11)];
for (let i = 0; i < owners.length; i++) {
    owners[i].tag = { value: 'owner-' + i };
    Object.defineProperty(owners[i], 'hidden', { get() { return this.tag.value; }, configurable: true });
}
for (let i = 0; i < 6; i++) {
    const garbage: any[] = [];
    for (let j = 0; j < 4000; j++) garbage.push({ j, text: 'exotic-' + j });
}
for (const owner of owners) {
    console.log(owner.tag.value, owner.hidden, Object.keys(owner).join(','));
    delete owner.tag;
    console.log(Object.hasOwn(owner, 'tag'));
}
console.log((new Temporal.PlainDate(2026, 10, 11) as any).tag === undefined);
