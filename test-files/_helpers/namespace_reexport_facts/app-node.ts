import { LayerNode } from "./layer-node.ts";

const t = LayerNode.tags(["global", "location"]);
export const makeGlobalNode = t.make("global");
export const makeLocationNode = t.make("location");
