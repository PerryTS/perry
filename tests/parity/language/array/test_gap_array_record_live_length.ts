function consume(a: any, grow: boolean) {
  const got: string[] = [];
  for (const x of a) {
    got.push(x.s);
    if (got.length === 1) {
      if (grow) for (let i = 3; i < 80; i++) a.push({s: String(i)});
      else a.length = 1;
    }
  }
  return got.join(",");
}
for (let warm = 0; warm < 4; warm++) {
  console.log("grow", consume([{s:"0"},{s:"1"},{s:"2"}], true));
  console.log("shrink", consume([{s:"0"},{s:"1"},{s:"2"}], false));
}
const sparse: any = [{s:"a"}, {s:"b"}];
Object.defineProperty(sparse, "0", {get(){ sparse.length = 1; return {s:"getter"}; }});
console.log("descriptor", consume(sparse, false));
