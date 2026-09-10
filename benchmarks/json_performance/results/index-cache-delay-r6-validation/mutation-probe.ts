function read(rows: any, i: number): any { return rows[i].id; }
let source = "[";
for (let i = 0; i < 128; i++) {
    if (i > 0) source += ",";
    source += '{"id":' + String(i) + ',"name":"user_name","nested":{"x":1}}';
}
source += "]";
const rows: any = JSON.parse(source);
const mode = process.argv[2];
if (mode === "index-data") Object.defineProperty(rows, "0", { value: {id: 99} });
if (mode === "index-getter") Object.defineProperty(rows, "0", { get() { return {id: 98}; } });
if (mode === "record-data") rows[0].id = 97;
if (mode === "record-getter") Object.defineProperty(rows[0], "id", { get() { return 96; } });
if (mode === "delete-index") {
    delete rows[0];
    Object.defineProperty(Array.prototype, "0", { get() { return {id: 95}; }, configurable: true });
}
if (mode === "delete-field") {
    delete rows[0].id;
    Object.defineProperty(Object.prototype, "id", { get() { return 94; }, configurable: true });
}
if (mode === "index-write") rows[0] = {id: 93};
if (mode === "prototype") Object.setPrototypeOf(rows, {0: {id: 92}});
console.log(read(rows, 0));
if (mode === "delete-index") delete (Array.prototype as any)[0];
if (mode === "delete-field") delete (Object.prototype as any).id;
