import * as url from "node:url";

function show(label: string, value: unknown) {
  console.log(label + ":", String(value));
}

function showJson(label: string, value: unknown) {
  console.log(label + ":", JSON.stringify(value));
}

const LegacyUrl = (url as any).Url;
const resolveObject = (url as any).resolveObject;

show("keys has Url", Object.keys(url).includes("Url"));
show("keys has resolveObject", Object.keys(url).includes("resolveObject"));
show("Url typeof", typeof LegacyUrl);
show("Url name", LegacyUrl && LegacyUrl.name);
show("Url length", LegacyUrl && LegacyUrl.length);
show("resolveObject typeof", typeof resolveObject);
show("resolveObject name", resolveObject && resolveObject.name);
show("resolveObject length", resolveObject && resolveObject.length);

const legacy = new url.Url();
showJson("new Url keys first10", Object.keys(legacy).slice(0, 10));
showJson(
  "new Url values first5",
  Object.keys(legacy).slice(0, 5).map((key) => [key, legacy[key]]),
);

const resolved = resolveObject("http://a/b?x=1#h", "../c?y=2");
show("resolveObject href", resolved.href);
show("resolveObject pathname", resolved.pathname);
show("resolveObject query type", typeof resolved.query);
showJson("resolveObject keys first8", Object.keys(resolved).slice(0, 8));
