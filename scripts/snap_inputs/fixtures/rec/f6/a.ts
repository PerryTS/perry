// SNAPCUT2 F6 a (static dependency): dead temporaries of one size class (kept briefly so they are allocated, then dropped).
let keep: any[] = [];
for (let i = 0; i < 200000; i++) { keep.push([i, i, i, i]); if (keep.length > 1000) keep = []; }
export const a = 1;
