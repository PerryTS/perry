// SNAPCUT2 F6 b (imaged): genuine holds of E, allocated into reused cells.
export const holders: any[] = [];
for (let i = 0; i < 1000; i++) holders.push([(globalThis as any).E, 0, 0, 0]);
