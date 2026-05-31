import * as Module from "node:module";

console.log("namespace object:", typeof Module);
console.log("constants object:", typeof Module.constants);
console.log("compile status enabled:", Module.constants.compileCacheStatus.ENABLED);
console.log("enable type length:", typeof Module.enableCompileCache, Module.enableCompileCache.length);
console.log("get dir type length:", typeof Module.getCompileCacheDir, Module.getCompileCacheDir.length);
console.log("flush type length:", typeof Module.flushCompileCache, Module.flushCompileCache.length);
console.log("findSourceMap type length:", typeof Module.findSourceMap, Module.findSourceMap.length);
console.log("SourceMap type length:", typeof Module.SourceMap, Module.SourceMap.length);

const capturedFindSourceMap = Module.findSourceMap;
console.log("captured find missing:", capturedFindSourceMap("/tmp/perry-no-source-map.js") === undefined);
