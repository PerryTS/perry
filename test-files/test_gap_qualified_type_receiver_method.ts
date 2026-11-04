// A receiver annotated with a namespace-qualified type (`ns.Service`) calls
// its own `join`/`find`, not the Array fast path those names share.
import type * as Svc from "./_helpers/qualified_type_receiver/service.ts";

const service: Svc.Service = {
  sep: "/",
  join: (...segments) => segments.join("/"),
  find: (needle) => `found:${needle}`,
};
const boxed: Svc.Box<string> = { join: (...segments) => segments.join("+") };

function viaQualified(svc: Svc.Service, root: string): string[] {
  const dir = svc.join(root, "supabase");
  return [dir, svc.join(dir, "config.toml"), svc.find("x"), svc.sep];
}

function viaQualifiedGeneric(box: Svc.Box<string>): string {
  return box.join("a", "b", "c");
}

console.log(JSON.stringify(viaQualified(service, "/w")));
console.log(viaQualifiedGeneric(boxed));
