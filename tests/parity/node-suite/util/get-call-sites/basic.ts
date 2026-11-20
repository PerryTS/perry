import util, { getCallSites } from "node:util";

function show(label: string, fn: () => unknown) {
  try {
    console.log(`${label}:`, fn());
  } catch (err) {
    const e = err as NodeJS.ErrnoException;
    console.log(`${label} throw:`, e.name, e.code, e.message.split("\n")[0]);
  }
}

function summarize(label: string, sites: any[]) {
  const first = sites[0];
  console.log(`${label} array:`, Array.isArray(sites));
  console.log(`${label} length:`, sites.length);
  console.log(`${label} keys:`, Object.keys(first).sort().join(","));
  console.log(
    `${label} types:`,
    [
      typeof first.functionName,
      typeof first.scriptName,
      typeof first.scriptId,
      typeof first.lineNumber,
      typeof first.columnNumber,
      typeof first.column,
    ].join(","),
  );
  console.log(`${label} column alias:`, first.column === first.columnNumber);
  console.log(`${label} positive positions:`, first.lineNumber >= 1 && first.columnNumber >= 1);
}

function probe() {
  summarize("namespace", util.getCallSites(3));
  summarize("named", getCallSites(1, { sourceMap: false }));
}

probe();

show("default no args", () => {
  const sites = util.getCallSites();
  return Array.isArray(sites) && sites.length > 0 && sites.length <= 10;
});
show("object options", () => {
  const sites = util.getCallSites({ sourceMap: false });
  return Array.isArray(sites) && sites.length > 0 && sites.length <= 10;
});
show("sourceMap true", () => util.getCallSites(1, { sourceMap: true }).length);

show("frame zero", () => util.getCallSites(0).length);
show("frame negative", () => util.getCallSites(-1).length);
show("frame fractional", () => util.getCallSites(3.6).length);
show("frame string", () => util.getCallSites("x" as any).length);
show("options null", () => util.getCallSites(1, null as any).length);
show("sourceMap string", () => util.getCallSites(1, { sourceMap: "x" as any }).length);
