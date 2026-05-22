for (const input of [{ a: "1", b: "2" }, [["a", "1"], ["a", "2"]], new URLSearchParams("x=1")] as any[]) {
  try { const p = new URLSearchParams(input); console.log("params:", p.toString()); } catch (err: any) { console.log("params err:", err?.name, err?.code || "no-code"); }
}
try { new URLSearchParams([["a"]] as any); console.log("bad tuple no throw"); } catch (err: any) { console.log("bad tuple:", err?.name, err?.code || "no-code"); }
