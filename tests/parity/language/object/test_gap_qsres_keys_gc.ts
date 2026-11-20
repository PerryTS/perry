// Allocating loops make the protected moving-GC gate non-vacuous.
let sum = 0;
let names = '';
for (let i = 0; i < 160; i++) {
    const value: any = { qsres_long_first: 'value' + i, qsres_long_second: i, qsres_long_third: i + 1 };
    const keys = Object.keys(value);
    const pressure: any[] = [];
    for (let j = 0; j < 8; j++) pressure.push({ text: 'pressure' + i + ':' + j, next: [i, j] });
    sum += keys.length + pressure.length;
    names = keys.join('|');
    keys[0] += '_changed';
    if (Object.keys(value)[0] !== 'qsres_long_first') throw new Error('key word mutated');
    const dictionary: any = {};
    dictionary['qsres_dynamic_' + (i % 3)] = i;
    dictionary['10'] = i;
    dictionary['2'] = i;
    const ordered = Object.keys(dictionary);
    if (ordered[0] !== '2' || ordered[1] !== '10') throw new Error('key order');
    sum += ordered.length;
    // No receiver survives beyond the call: only the returned keys do.
    sum += Object.keys({ qsres_temporary_first: i, qsres_temporary_second: i + 1 }).length;
}
console.log(sum);
console.log(names);
