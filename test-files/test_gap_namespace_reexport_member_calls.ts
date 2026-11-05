// Calls through re-exported namespaces (`export * as X from "./x"`, a named
// re-export, `export *`) reach the module's functions with the caller's
// object literals. opencode v2.1.2: the bus layer node read its `deps` array
// as its layer ("A layer node requires an Effect Layer"), and the skill
// plugin's Skill.Info.make returned a function (SchemaError).
import { makeGlobalNode, makeLocationNode } from "./_helpers/namespace_reexport_facts/app-node.ts";
import { LayerNode } from "./_helpers/namespace_reexport_facts/layer-node.ts";
import { Nodes, makeNode, tags } from "./_helpers/namespace_reexport_facts/barrel.ts";

const show = (label: string, p: { info: { name: string | undefined; implementation: unknown; dependencies: unknown[] }; tag: string | undefined }) =>
  console.log(label, p.info.name, JSON.stringify(p.info.implementation), p.info.dependencies.length, String(p.tag));

const db = makeGlobalNode({ service: { key: "Db" }, layer: { l: "db" }, deps: [] });
const bus = makeGlobalNode({ service: { key: "Bus" }, deps: [db], layer: { l: "bus" } });
const loc = makeLocationNode({ deps: [db, bus], layer: { l: "loc" }, name: "Loc" });
show("db", db);
show("bus", bus);
show("loc", loc);
show("replace", bus.replace({ l: "r" }));
show("ns.make", LayerNode.make({ deps: [db], tag: "t", layer: { l: "m" }, name: "M" }));
show("barrel.ns", Nodes.make({ deps: [db, bus, loc], layer: { l: "n" }, name: "N" }));
show("barrel.named", makeNode({ deps: [loc], name: "K", layer: { l: "k" } }));
show("barrel.all", tags(["x"]).make("x")({ deps: [bus], layer: { l: "x" }, service: { key: "X" } }));
