function fail() { throw new Error("caught"); }
try {
  try { fail(); } finally { console.log("finally"); }
} catch (error) {
  console.log(error.message);
}
console.log("continued");
