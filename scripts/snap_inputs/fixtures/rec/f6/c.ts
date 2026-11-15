// SNAPCUT2 F6 c (static dependency after a): churn, so collections run and the cells of a are reused later.
let keep: any[] = [];
for (let i = 0; i < 400000; i++) { keep.push([i, i, i, i]); if (keep.length > 1000) keep = []; }
export const c = 1;
