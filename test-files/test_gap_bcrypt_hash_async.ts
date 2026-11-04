import bcrypt from "bcrypt";
const h = await bcrypt.hash("pw", 10);
const matches = await bcrypt.compare("pw", h);
const mismatch = await bcrypt.compare("no", h);
if (matches !== true || mismatch !== false) {
    throw new Error("bcrypt.compare must resolve to booleans");
}
console.log("hash", h.length, h.slice(0, 7), matches, mismatch);
const s = bcrypt.hashSync("pw", 4), salt = await bcrypt.genSalt(5), saltS = bcrypt.genSaltSync(6);
console.log("sync", s.length, s.slice(0, 7), bcrypt.compareSync("pw", s), bcrypt.compareSync("no", s));
console.log("salt", salt.length, salt.slice(0, 7), saltS.length, saltS.slice(0, 7));
