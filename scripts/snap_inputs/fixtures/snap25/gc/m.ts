const a: object[] = []; for (let i = 0; i < 400000; i++) a.push({ i, s: "x" + i }); export const v = a.length;
