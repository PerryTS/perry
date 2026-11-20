// Objects held only through a module global: live.
export const held = Array.from({ length: 3000 }, (_, i) => ({ id: i, name: "n" + i, tags: [i] }));
