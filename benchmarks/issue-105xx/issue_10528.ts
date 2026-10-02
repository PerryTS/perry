// fast-json-stringify-shaped serializer: source generated at RUNTIME from a schema and run through `new Function`
// ("interp") vs the identical code written in the program ("static")
const variant = process.argv[2] || "interp"; const N = Number(process.argv[3] || "20000");
const schema = JSON.parse('{"id":"integer","name":"string","email":"string","active":"boolean"}');
let code = "let json = '{'; let sep = '';\n";
for (const k of Object.keys(schema)) {
  const read = schema[k] === "integer" ? `asInteger(obj.${k})` : schema[k] === "string" ? `asString(obj.${k})` : `(obj.${k} ? 'true' : 'false')`;
  code += `json += sep + '"${k}":' + ${read}; sep = ',';\n`;
}
code += "const tags = obj.tags; json += ',\"tags\":[';\nfor (let i = 0; i < tags.length; i++) { if (i > 0) json += ','; json += asString(tags[i]); }\nreturn json + ']}';";
function asString(s: string): string { return JSON.stringify(s); }
function asInteger(n: number): string { return String(Math.trunc(n)); }
const generated = new Function("obj", "asInteger", "asString", code) as (o: any, a: any, b: any) => string;
function compiled(obj: any, asInteger: any, asString: any): string {
  let json = '{'; let sep = '';
  json += sep + '"id":' + asInteger(obj.id); sep = ','; json += sep + '"name":' + asString(obj.name); sep = ',';
  json += sep + '"email":' + asString(obj.email); sep = ','; json += sep + '"active":' + (obj.active ? 'true' : 'false'); sep = ',';
  const tags = obj.tags; json += ',"tags":[';
  for (let i = 0; i < tags.length; i++) { if (i > 0) json += ','; json += asString(tags[i]); }
  return json + ']}';
}
const f = variant === "static" ? compiled : generated;
const user = { id: 42, name: "Ada Lovelace", email: "ada@example.com", active: true, tags: ["admin", "math"] };
function run(n: number): number { let len = 0; for (let i = 0; i < n; i++) len += f(user, asInteger, asString).length; return len; }
run(N / 5 | 0); const t0 = performance.now(); const cs = run(N);
console.log(`variant=${variant} checksum=${cs} ms=${(performance.now() - t0).toFixed(1)} sample=${f(user, asInteger, asString)}`);