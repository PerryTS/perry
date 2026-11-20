export const table: { id: number; name: string }[] = [];
for (let i = 0; i < 200; i++) table.push({ id: i + 1, name: "n" + i });
export const words: string[] = [];
for (let i = 0; i < 100; i++) words.push("w" + i);
export const nested = { list: [1, 2, 3], inner: { k: "v" as unknown } };
