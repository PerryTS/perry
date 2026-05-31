// Gap test for #3683 (node:constants tail), #3677 (zlib Zstd enumeration),
// #3678 (util.types predicate tail). Byte-compared against
// `node --experimental-strip-types`.
import * as constants from "node:constants";
import * as zlib from "node:zlib";
import * as util from "node:util";

const c = constants as any;

// ── #3683: POSIX file flags, libuv, default cipher metadata ──
console.log("UV_DIRENT_UNKNOWN:", c.UV_DIRENT_UNKNOWN);
console.log("UV_DIRENT_FILE:", c.UV_DIRENT_FILE);
console.log("UV_DIRENT_DIR:", c.UV_DIRENT_DIR);
console.log("UV_DIRENT_LINK:", c.UV_DIRENT_LINK);
console.log("UV_DIRENT_BLOCK:", c.UV_DIRENT_BLOCK);
console.log("UV_FS_SYMLINK_DIR:", c.UV_FS_SYMLINK_DIR);
console.log("UV_FS_SYMLINK_JUNCTION:", c.UV_FS_SYMLINK_JUNCTION);
console.log("UV_FS_COPYFILE_EXCL:", c.UV_FS_COPYFILE_EXCL);
console.log("UV_FS_COPYFILE_FICLONE:", c.UV_FS_COPYFILE_FICLONE);
console.log("UV_FS_COPYFILE_FICLONE_FORCE:", c.UV_FS_COPYFILE_FICLONE_FORCE);
console.log("S_IFMT:", c.S_IFMT);
console.log("S_IFREG:", c.S_IFREG);
console.log("S_IFDIR:", c.S_IFDIR);
console.log("S_IFLNK:", c.S_IFLNK);
console.log("S_IFSOCK:", c.S_IFSOCK);
console.log("O_DIRECTORY:", c.O_DIRECTORY);
console.log("O_NOCTTY:", c.O_NOCTTY);
console.log("O_NONBLOCK:", c.O_NONBLOCK);
console.log("O_SYNC:", c.O_SYNC);
console.log("O_DSYNC:", c.O_DSYNC);
console.log("O_SYMLINK:", c.O_SYMLINK);
console.log("defaultCoreCipherList type:", typeof c.defaultCoreCipherList);
console.log("defaultCoreCipherList len:", (c.defaultCoreCipherList as string).length);
const constKeys = Object.keys(c);
console.log("constants has S_IFMT key:", constKeys.includes("S_IFMT"));
console.log("constants has UV_DIRENT_FILE key:", constKeys.includes("UV_DIRENT_FILE"));
console.log("constants has defaultCoreCipherList key:", constKeys.includes("defaultCoreCipherList"));

// ── #3677: zlib Zstd constants enumeration ──
const z = zlib.constants as any;
console.log("ZSTD_COMPRESS:", z.ZSTD_COMPRESS);
console.log("ZSTD_DECOMPRESS:", z.ZSTD_DECOMPRESS);
console.log("ZSTD_CLEVEL_DEFAULT:", z.ZSTD_CLEVEL_DEFAULT);
console.log("ZSTD_c_strategy:", z.ZSTD_c_strategy);
console.log("ZSTD_error_no_error:", z.ZSTD_error_no_error);
console.log("ZSTD_error_GENERIC:", z.ZSTD_error_GENERIC);
const zstdKeys = Object.keys(zlib.constants || {})
  .filter((k) => k.includes("ZSTD"))
  .sort();
console.log("zlib ZSTD count:", zstdKeys.length);
console.log("zlib ZSTD sample:", JSON.stringify(zstdKeys.slice(0, 8)));
console.log("zlib total keys:", Object.keys(zlib.constants).length);

// ── #3678: util.types predicate tail ──
console.log("isDataView type:", typeof util.types.isDataView);
console.log("isFloat16Array type:", typeof util.types.isFloat16Array);
console.log("isWeakMap type:", typeof util.types.isWeakMap);
console.log("isWeakSet type:", typeof util.types.isWeakSet);
console.log("isExternal type:", typeof util.types.isExternal);

console.log("isDataView(dv):", util.types.isDataView(new DataView(new ArrayBuffer(8))));
console.log("isDataView(buf):", util.types.isDataView(new ArrayBuffer(8)));
console.log("isFloat16Array(f16):", util.types.isFloat16Array(new Float16Array(2)));
console.log("isFloat16Array(f32):", util.types.isFloat16Array(new Float32Array(2)));
console.log("isWeakMap(wm):", util.types.isWeakMap(new WeakMap()));
console.log("isWeakMap(m):", util.types.isWeakMap(new Map()));
console.log("isWeakSet(ws):", util.types.isWeakSet(new WeakSet()));
console.log("isWeakSet(s):", util.types.isWeakSet(new Set()));
console.log("isExternal(obj):", util.types.isExternal({}));
console.log("isExternal(num):", util.types.isExternal(5));
