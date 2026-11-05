const input: Array<{ id: number; label?: string; rest?: number[] }> = [
  { id: 1, label: "a", rest: [2, 3] },
  { id: 2 },
];

const out = input.map(({ id, label = "n" + id, rest = [] }) => {
  return label + ":" + rest.length;
});

console.log("syntax/destructuring-defaults:" + out.join(","));
