// #10175: computed reads and enumeration must agree across the old 10k cutoff.
function parseWide(count: number, records: boolean): any {
  const parts: string[] = [];
  for (let i = 0; i < count; i++) {
    const value = records ? '{"id":' + i + ',"name":"v' + i + '"}' : String(i);
    parts.push('"k' + i + '":' + value);
  }
  return JSON.parse("{" + parts.join(",") + "}");
}

for (const count of [9999, 10000, 10001, 12000]) {
  for (const records of [false, true]) {
    const value = parseWide(count, records);
    const entries = Object.entries(value);
    const values = Object.values(value);
    console.log("wide", count, records, Object.keys(value).length, entries.length, values.length);
    for (const index of [0, 5, Math.floor(count / 2), count - 1]) {
      const key = "k" + index;
      console.log(key, JSON.stringify(value[key]), JSON.stringify(entries[index]),
        JSON.stringify(values[index]), key in value, Object.hasOwn(value, key));
    }
    console.log("literal", JSON.stringify(value["k5"]), "missing", value["k" + count]);
    const last = "k" + (count - 1);
    delete value[last];
    value[last] = 123;
    value["extra"] = 456;
    console.log("mutated", value[last], value["extra"], Object.keys(value).length);
  }
}
