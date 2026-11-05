function show(label: string, input: any) {
  try {
    const params = new URLSearchParams(input);
    console.log(label + ":", params.toString());
  } catch (err: any) {
    console.log(label + " err:", err?.name, err?.code || "no-code");
  }
}

show("map", new Map([["m", "n"]]));
show("set pair", [new Set(["x", "y"])]);
show("top set", new Set([["s", "t"]]));
show("array pairs", [["a", "1"], ["b", "2"]]);

for (const [label, input] of [
  ["number entry", [1]],
  ["object entry", [{}]],
  ["null entry", [null]],
  ["short tuple", [["a"]]],
  ["long tuple", [["a", "b", "c"]]],
  ["string entry", ["ab"]],
  ["short set", [new Set(["only"])]],
  ["long set", [new Set(["a", "b", "c"])]],
] as [string, any][]) {
  show(label, input);
}
