// A top-level await leaf in a program that imports a Worker.
export const record = { evals: 1 };
await 0;
export const settled = record.evals + 1;
