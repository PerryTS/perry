import ModuleDefault, * as ModuleNS from "node:module";
import { Module, _cache, _extensions, _pathCache, globalPaths } from "node:module";

console.log("Module function:", typeof Module, Module.name, Module.length);
console.log("default equals Module:", ModuleDefault === Module);
console.log("namespace default equals Module:", (ModuleNS as any).default === Module);
console.log("Module cache same:", (Module as any)._cache === _cache);
console.log("Module pathCache same:", (Module as any)._pathCache === _pathCache);
console.log("Module extensions same:", (Module as any)._extensions === _extensions);
console.log("Module globalPaths same:", (Module as any).globalPaths === globalPaths);
console.log("globalPaths array:", Array.isArray(globalPaths), globalPaths.length > 0);
console.log("extensions keys:", Object.keys(_extensions).sort().join(","));

const created = new Module("/tmp/perry-module-parent.js");
console.log("instance id:", created.id === "/tmp/perry-module-parent.js");
console.log("instance path type:", typeof created.path);
console.log("instance exports keys:", Object.keys(created.exports).length);
console.log("instance filename null:", created.filename === null);
console.log("instance loaded:", created.loaded);
console.log("instance children:", Array.isArray(created.children), created.children.length);
console.log("instance paths array:", Array.isArray((created as any).paths));

_cache.__perryProbe = { exports: { ok: 1 } };
console.log("cache mutable:", (Module as any)._cache.__perryProbe.exports.ok);
delete _cache.__perryProbe;

_pathCache.__perryPathProbe = "resolved.js";
console.log("pathCache mutable:", (Module as any)._pathCache.__perryPathProbe);
delete _pathCache.__perryPathProbe;

console.log("extension js typeof:", typeof _extensions[".js"]);
console.log("extension custom before:", String(_extensions[".perry"]));
_extensions[".perry"] = () => undefined;
console.log("extension custom after:", typeof (Module as any)._extensions[".perry"]);
delete _extensions[".perry"];
