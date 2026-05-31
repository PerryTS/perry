// process.loadEnvFile(path?) loads a .env file (Node 20.12+).
import { Buffer } from "node:buffer";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import * as url from "node:url";

console.log("is function:", typeof process.loadEnvFile === "function");

const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "perry-load-env-file-"));
const keys = [
  "PERRY_LOAD_ENV_EXPORT",
  "PERRY_LOAD_ENV_INLINE",
  "PERRY_LOAD_ENV_HASH",
  "PERRY_LOAD_ENV_MULTI",
  "PERRY_LOAD_ENV_BUFFER",
  "PERRY_LOAD_ENV_URL",
];
for (const key of keys) delete process.env[key];

try {
  const pathFile = path.join(tmp, "path.env");
  fs.writeFileSync(
    pathFile,
    "export PERRY_LOAD_ENV_EXPORT=works\n" +
      "PERRY_LOAD_ENV_INLINE=one # comment\n" +
      "PERRY_LOAD_ENV_HASH=\"two # hash\"\n" +
      "PERRY_LOAD_ENV_MULTI=\"line1\nline2\"\n",
  );
  process.loadEnvFile(pathFile);
  console.log("export:", process.env.PERRY_LOAD_ENV_EXPORT);
  console.log("inline:", process.env.PERRY_LOAD_ENV_INLINE);
  console.log("hash:", process.env.PERRY_LOAD_ENV_HASH);
  console.log("multi:", JSON.stringify(process.env.PERRY_LOAD_ENV_MULTI));

  const bufferFile = path.join(tmp, "buffer.env");
  fs.writeFileSync(bufferFile, "PERRY_LOAD_ENV_BUFFER=buffer-path\n");
  process.loadEnvFile(Buffer.from(bufferFile));
  console.log("buffer:", process.env.PERRY_LOAD_ENV_BUFFER);

  const urlFile = path.join(tmp, "url.env");
  fs.writeFileSync(urlFile, "PERRY_LOAD_ENV_URL=file-url\n");
  process.loadEnvFile(url.pathToFileURL(urlFile));
  console.log("url:", process.env.PERRY_LOAD_ENV_URL);

  try {
    process.loadEnvFile(1 as any);
    console.log("invalid:", "NO_THROW");
  } catch (err: any) {
    console.log("invalid:", err.name, err.code, err.message.split("\n")[0]);
  }
} finally {
  fs.rmSync(tmp, { recursive: true, force: true });
  for (const key of keys) delete process.env[key];
}
