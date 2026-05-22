import { URL } from "node:url";

for (const [input, base] of [["/x", "https://example.com"], ["http://[::1]", undefined], ["http://%zz", undefined], ["x", "bad base"]] as any[]) {
  try { console.log("canParse:", input, base, URL.canParse(input, base)); } catch (err: any) { console.log("canParse err:", err?.name, err?.code || "no-code"); }
  try { const u: any = (URL as any).parse(input, base); console.log("parse:", u && u.href); } catch (err: any) { console.log("parse err:", err?.name, err?.code || "no-code"); }
}
