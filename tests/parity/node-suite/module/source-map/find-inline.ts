import {
  createRequire,
  findSourceMap,
  getSourceMapsSupport,
  setSourceMapsSupport,
} from "node:module";

const req = createRequire(import.meta.url);
const filename = req.resolve("./fixtures/inline-map.cjs");
const previous = getSourceMapsSupport();
const normalize = (value: unknown) =>
  JSON.stringify(value).replaceAll(process.cwd(), "<cwd>");
try {
  setSourceMapsSupport(true, { nodeModules: true, generatedCode: true });
  req("./fixtures/inline-map.cjs");
  const map = findSourceMap(filename);
  console.log("found:", map !== undefined);
  console.log(
    "payload:",
    map!.payload.version,
    map!.payload.file,
    normalize(map!.payload.sources[0]),
    map!.payload.names[0],
  );
  console.log("entry:", normalize(map!.findEntry(0, 0)));
  console.log("same lookup:", findSourceMap(filename) === map);
  console.log("missing:", String(findSourceMap(filename + ".missing")));
} finally {
  setSourceMapsSupport(previous.enabled, {
    nodeModules: previous.nodeModules,
    generatedCode: previous.generatedCode,
  });
  delete req.cache[filename];
}
