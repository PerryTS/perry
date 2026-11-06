for (const input of [{ a: "1", b: "2" }, [["a", "1"], ["a", "2"]], new URLSearchParams("x=1")] as any[]) {
  try { const p = new URLSearchParams(input); console.log("params:", p.toString()); } catch (err: any) { console.log("params err:", err?.name, err?.code || "no-code"); }
}
for (const bad of [[["a"]], [[]], [["a", "1", "extra"]]] as any[]) {
  try { new URLSearchParams(bad); console.log("bad tuple no throw:", JSON.stringify(bad)); } catch (err: any) { console.log("bad tuple:", err?.name, err?.code || "no-code"); }
}
