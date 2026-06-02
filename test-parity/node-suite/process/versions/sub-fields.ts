// process.versions includes the bundled-dependency versions beyond `node`.
console.log("uv:", typeof process.versions.uv === "string");
console.log("modules:", typeof process.versions.modules === "string");
