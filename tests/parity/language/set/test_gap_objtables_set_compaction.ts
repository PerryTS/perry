const set = new Set<number>();
for (let i = 0; i < 80; i++) set.add(i);
const seen: number[] = [];
for (const value of set) {
    seen.push(value);
    if (value === 4) {
        set.clear();
        set.add(100);
        set.add(101);
        const garbage: any[] = [];
        for (let i = 0; i < 12000; i++) garbage.push({ i, text: 'set-' + i });
    }
}
console.log(seen.join(','), [...set].join(','));
const fresh = new Set([7, 8]);
console.log([...fresh].join(','));
