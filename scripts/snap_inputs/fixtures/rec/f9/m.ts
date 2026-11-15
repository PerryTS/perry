const a: any[] = [];
for (let i = 0; i < 300000; i++) a.push({ i });
export const held = [(globalThis as any).x];
