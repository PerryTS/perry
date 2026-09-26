import * as zlib from "node:zlib";
const input = Buffer.from("hello hello hello hello tokio-free world");
zlib.gzip(input, (err, gz) => {
  if (err) throw err;
  zlib.gunzip(gz, (err2, out) => {
    if (err2) throw err2;
    console.log("gzip roundtrip", out.toString());
    zlib.deflate(input, (e3, df) => {
      zlib.inflate(df, (e4, back) => console.log("deflate roundtrip", back.toString() === input.toString()));
    });
  });
});
