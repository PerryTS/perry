import constantsDefault from "node:constants";
import * as constantsNs from "node:constants";
import {
  O_DIRECTORY,
  O_DIRECT,
  O_DSYNC,
  O_NOATIME,
  O_NOCTTY,
  O_NONBLOCK,
  O_SYNC,
  S_IFBLK,
  S_IFCHR,
  S_IFDIR,
  S_IFIFO,
  S_IFLNK,
  S_IFMT,
  S_IFREG,
  S_IFSOCK,
  S_IRWXG,
  S_IRWXO,
  S_IRWXU,
  UV_DIRENT_BLOCK,
  UV_DIRENT_CHAR,
  UV_DIRENT_DIR,
  UV_DIRENT_FIFO,
  UV_DIRENT_FILE,
  UV_DIRENT_LINK,
  UV_DIRENT_SOCKET,
  UV_DIRENT_UNKNOWN,
  UV_FS_O_FILEMAP,
  UV_FS_COPYFILE_EXCL,
  UV_FS_COPYFILE_FICLONE,
  UV_FS_COPYFILE_FICLONE_FORCE,
  UV_FS_SYMLINK_DIR,
  UV_FS_SYMLINK_JUNCTION,
  defaultCoreCipherList,
} from "node:constants";
import fs from "node:fs";

const numericNames = [
  "UV_DIRENT_UNKNOWN",
  "UV_DIRENT_FILE",
  "UV_DIRENT_DIR",
  "UV_DIRENT_LINK",
  "UV_DIRENT_FIFO",
  "UV_DIRENT_SOCKET",
  "UV_DIRENT_CHAR",
  "UV_DIRENT_BLOCK",
  "UV_FS_O_FILEMAP",
  "UV_FS_SYMLINK_DIR",
  "UV_FS_SYMLINK_JUNCTION",
  "UV_FS_COPYFILE_EXCL",
  "UV_FS_COPYFILE_FICLONE",
  "UV_FS_COPYFILE_FICLONE_FORCE",
  "S_IFMT",
  "S_IFREG",
  "S_IFDIR",
  "S_IFCHR",
  "S_IFBLK",
  "S_IFIFO",
  "S_IFLNK",
  "S_IFSOCK",
  "S_IRWXU",
  "S_IRWXG",
  "S_IRWXO",
  "O_DIRECTORY",
  "O_DIRECT",
  "O_NOCTTY",
  "O_NOATIME",
  "O_NONBLOCK",
  "O_SYNC",
  "O_DSYNC",
];

const namedValues = [
  UV_DIRENT_UNKNOWN,
  UV_DIRENT_FILE,
  UV_DIRENT_DIR,
  UV_DIRENT_LINK,
  UV_DIRENT_FIFO,
  UV_DIRENT_SOCKET,
  UV_DIRENT_CHAR,
  UV_DIRENT_BLOCK,
  UV_FS_O_FILEMAP,
  UV_FS_SYMLINK_DIR,
  UV_FS_SYMLINK_JUNCTION,
  UV_FS_COPYFILE_EXCL,
  UV_FS_COPYFILE_FICLONE,
  UV_FS_COPYFILE_FICLONE_FORCE,
  S_IFMT,
  S_IFREG,
  S_IFDIR,
  S_IFCHR,
  S_IFBLK,
  S_IFIFO,
  S_IFLNK,
  S_IFSOCK,
  S_IRWXU,
  S_IRWXG,
  S_IRWXO,
  O_DIRECTORY,
  O_DIRECT,
  O_NOCTTY,
  O_NOATIME,
  O_NONBLOCK,
  O_SYNC,
  O_DSYNC,
];

for (const name of numericNames) {
  const value = (constantsDefault as Record<string, unknown>)[name];
  const nsValue = (constantsNs as Record<string, unknown>)[name];
  console.log(`${name}:`, typeof value, value, nsValue === value);
}

console.log(
  "named imports match:",
  namedValues.every((value, index) => value === (constantsDefault as any)[numericNames[index]]),
);
console.log("fs S_IFDIR same:", constantsDefault.S_IFDIR === fs.constants.S_IFDIR);
console.log("fs O_DIRECTORY same:", constantsDefault.O_DIRECTORY === fs.constants.O_DIRECTORY);
console.log("O_SYMLINK dynamic:", typeof (constantsDefault as any).O_SYMLINK);
console.log(
  "keys include tail:",
  numericNames.every((name) => Object.keys(constantsDefault).includes(name)),
);
console.log(
  "defaultCoreCipherList:",
  typeof defaultCoreCipherList,
  defaultCoreCipherList.length,
  defaultCoreCipherList === constantsDefault.defaultCoreCipherList,
  Object.keys(constantsDefault).includes("defaultCoreCipherList"),
);
