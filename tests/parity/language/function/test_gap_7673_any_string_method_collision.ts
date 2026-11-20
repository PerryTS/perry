class Schema {
  trim(): Schema {
    return this;
  }

  split(_separator: string): Schema {
    return this;
  }

  min(_n: number): Schema {
    return this;
  }
}

function makeSchema(): any {
  return new Schema();
}

console.log("schema trim type:", typeof makeSchema().trim());
console.log("schema trim member:", typeof makeSchema().trim().min);
console.log("schema trim call:", makeSchema().trim().min(1) instanceof Schema);
console.log("schema split type:", typeof makeSchema().split(","));

function makeString(): any {
  // Keep the runtime value a string while preventing local literal inference
  // from turning this `any` fallback witness back into a proven string.
  return JSON.parse(JSON.stringify("  Ab|Cd  "));
}

const value: any = makeString();
console.log("trim:", value.trim());
console.log("trimStart:", JSON.stringify(value.trimStart()));
console.log("trimEnd:", JSON.stringify(value.trimEnd()));
console.log("split:", value.split("|").join(","));
console.log("charAt:", value.charAt(2));
console.log("charCodeAt:", value.charCodeAt(2));
console.log("codePointAt:", value.codePointAt(2));
console.log("substring:", value.substring(2, 4));
console.log("substr:", value.substr(2, 2));
console.log("lower:", value.toLowerCase());
console.log("upper:", value.toUpperCase());
console.log("locale lower:", value.toLocaleLowerCase("en-US"));
console.log("locale upper:", value.toLocaleUpperCase("en-US"));
console.log("replaceAll:", value.replaceAll(" ", "_"));
console.log("padStart:", value.trim().padStart(7, "."));
console.log("padEnd:", value.trim().padEnd(7, "."));
console.log("repeat:", value.trim().repeat(2));
console.log("localeCompare:", Math.sign(value.trim().localeCompare("Ab|Ce")));
console.log(
  "replace callback:",
  value.replace("Ab", (match: string) => match.toLowerCase()),
);
console.log("inline Any string:", makeString().trim());
