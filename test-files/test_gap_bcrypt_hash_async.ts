import bcrypt from "bcrypt";
const h = await bcrypt.hash("pw", 10);
console.log("hash", h.length, h.slice(0, 7), await bcrypt.compare("pw", h), await bcrypt.compare("no", h));
const s = bcrypt.hashSync("pw", 4), salt = await bcrypt.genSalt(5), saltS = bcrypt.genSaltSync(6);
console.log("sync", s.length, s.slice(0, 7), bcrypt.compareSync("pw", s), bcrypt.compareSync("no", s));
console.log("salt", salt.length, salt.slice(0, 7), saltS.length, saltS.slice(0, 7));
