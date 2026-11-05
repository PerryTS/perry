const u = new URL("https://example.com/a");
for (const [prop, value] of [["protocol", "1"], ["port", "99999"], ["hostname", "bad host"], ["href", "not a url"]] as any[]) {
  try { (u as any)[prop] = value; console.log("set:", prop, "=>", u.href); } catch (err: any) { console.log("set:", prop, err?.name, err?.code || "no-code"); }
}
