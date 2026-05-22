import os from "node:os";

for (const key of ["EEXIST", "ENOENT", "EINVAL", "ECONNRESET", "ETIMEDOUT"]) {
  const value = (os.constants.errno as any)[key];
  console.log(key + ":", typeof value, value !== 0);
}
