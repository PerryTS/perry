import url from "node:url";

for (const input of ["http://[::1]:8080/a?b=c", "http://user:pass@example.com/a", "//example.com/path", "http://%zz/"]) {
  try {
    const u = url.parse(input, true, true);
    console.log("parse:", input, "=>", u.protocol, u.host, u.pathname, typeof u.query);
  } catch (err: any) { console.log("parse:", input, "=>", err?.name, err?.code || "no-code"); }
}
