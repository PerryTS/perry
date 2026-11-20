const values = new Map<string, number>([
    ["a", 1],
    ["b", 2],
]);
const doubled = new Set([...values.values()].map((value) => value * 2));

console.log(`map-set:${values.get("b")}:${Array.from(doubled).join(",")}`);
