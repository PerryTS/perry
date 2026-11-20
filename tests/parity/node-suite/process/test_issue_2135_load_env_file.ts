// Refs #2135 (#1399 follow-through): `process.loadEnvFile(path?)` previously
// returned `undefined` as a no-op because `process.env.X = v` didn't persist.
// #1344 has since wired writes through `std::env::set_var`, so eager loading
// is meaningful — Perry now reads the .env file and merges `KEY=value` pairs
// into `process.env`, matching Node 20.12+.

import { writeFileSync, unlinkSync } from "node:fs";

const path = "/tmp/.env_perry_2135";
writeFileSync(
    path,
    "# a comment\n" +
        "KEY1=value1\n" +
        "KEY2=\"quoted value\"\n" +
        "KEY3='single quoted'\n" +
        "KEY4=with spaces\n" +
        "EMPTY=\n" +
        "NOQUOTE=raw=val\n" +
        "SHOULD_TRIM = trimmed\n",
);

process.loadEnvFile(path);
console.log("KEY1:", process.env.KEY1);
console.log("KEY2:", process.env.KEY2);
console.log("KEY3:", process.env.KEY3);
console.log("KEY4:", process.env.KEY4);
console.log("EMPTY:", JSON.stringify(process.env.EMPTY));
console.log("NOQUOTE:", process.env.NOQUOTE);
console.log("SHOULD_TRIM:", JSON.stringify(process.env.SHOULD_TRIM));

unlinkSync(path);

// Missing file → ENOENT/open
try {
    process.loadEnvFile("/no/such/.env_perry_2135");
    console.log("no throw");
} catch (e: any) {
    console.log(e.code, e.syscall);
}
