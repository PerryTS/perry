// A relative reference whose final segment is "." or ".." names a directory: its URL ends with "/".
const file = "file:///app/src/main.js";
const http = "http://example.com/a/b/c";
for (const base of [file, http]) {
  for (const reference of [".", "./", "..", "../", "./.", "../.", "x", "./x/", "../x", "../..", "./sub/.."]) {
    console.log(base, JSON.stringify(reference), new URL(reference, base).href);
  }
}
console.log(new URL(".", new URL(".", file)).href, new URL("z.txt", new URL(".", file)).href);
