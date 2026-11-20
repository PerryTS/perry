// A const array of object literals mutated in place stays a real array (#12742).
const s = [{ v: 1 }];
s.push({ v: 2 });
console.log(s.length, s[1].v);
const top = s.pop();
console.log(top!.v, s.length);
s.unshift({ v: 0 });
console.log(s[0].v, s.length);
const first = s.shift();
console.log(first!.v, s[0].v);

const t = [{ id: "a" }, { id: "b" }];
t.splice(1, 0, { id: "c" });
console.log(t.map((x) => x.id).join(","));

const stack = [{ node: 1, depth: 0 }];
const seen: number[] = [];
while (stack.length > 0) {
  const cur = stack.pop()!;
  seen.push(cur.node);
  if (cur.depth < 2) {
    stack.push({ node: cur.node * 2, depth: cur.depth + 1 });
    stack.push({ node: cur.node * 2 + 1, depth: cur.depth + 1 });
  }
}
console.log(seen.join(" "));
