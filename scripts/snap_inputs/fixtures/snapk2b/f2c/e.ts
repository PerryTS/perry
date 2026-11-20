// An eager module object with properties past its inline slots (a spill).
export const box: any = {};
for (let i = 0; i < 24; i++) box["k" + i] = i;
