//! The `fs` namespace: the export keys of `node:fs` in Node's order, and what
//! each one is. The realm producer reads this item as source text and births
//! its cell through the native namespace birth's reference
//! (`namespaces = native_namespaces`); the vtable it declares carries the
//! keys to `Object.keys`.
crate::realm_decl! {
    namespace NM_FS = "fs" {
        names = ["node:fs.default"];
        members = {
            appendFile = (ref("node:fs.appendFile"));
            appendFileSync = (ref("node:fs.appendFileSync"));
            access = (ref("node:fs.access"));
            accessSync = (ref("node:fs.accessSync"));
            chown = (ref("node:fs.chown"));
            chownSync = (ref("node:fs.chownSync"));
            chmod = (ref("node:fs.chmod"));
            chmodSync = (ref("node:fs.chmodSync"));
            close = (ref("node:fs.close"));
            closeSync = (ref("node:fs.closeSync"));
            copyFile = (ref("node:fs.copyFile"));
            copyFileSync = (ref("node:fs.copyFileSync"));
            cp = (ref("node:fs.cp"));
            cpSync = (ref("node:fs.cpSync"));
            createReadStream = (ref("node:fs.createReadStream"));
            createWriteStream = (ref("node:fs.createWriteStream"));
            exists = (ref("node:fs.exists"));
            existsSync = (ref("node:fs.existsSync"));
            fchown = (ref("node:fs.fchown"));
            fchownSync = (ref("node:fs.fchownSync"));
            fchmod = (ref("node:fs.fchmod"));
            fchmodSync = (ref("node:fs.fchmodSync"));
            fdatasync = (ref("node:fs.fdatasync"));
            fdatasyncSync = (ref("node:fs.fdatasyncSync"));
            fstat = (ref("node:fs.fstat"));
            fstatSync = (ref("node:fs.fstatSync"));
            fsync = (ref("node:fs.fsync"));
            fsyncSync = (ref("node:fs.fsyncSync"));
            ftruncate = (ref("node:fs.ftruncate"));
            ftruncateSync = (ref("node:fs.ftruncateSync"));
            futimes = (ref("node:fs.futimes"));
            futimesSync = (ref("node:fs.futimesSync"));
            glob = (ref("node:fs.glob"));
            globSync = (ref("node:fs.globSync"));
            lchown = (ref("node:fs.lchown"));
            lchownSync = (ref("node:fs.lchownSync"));
            lchmod = (undefined);
            lchmodSync = (undefined);
            link = (ref("node:fs.link"));
            linkSync = (ref("node:fs.linkSync"));
            lstat = (ref("node:fs.lstat"));
            lstatSync = (ref("node:fs.lstatSync"));
            lutimes = (ref("node:fs.lutimes"));
            lutimesSync = (ref("node:fs.lutimesSync"));
            mkdir = (ref("node:fs.mkdir"));
            mkdirSync = (ref("node:fs.mkdirSync"));
            mkdtemp = (ref("node:fs.mkdtemp"));
            mkdtempSync = (ref("node:fs.mkdtempSync"));
            mkdtempDisposableSync = (ref("node:fs.mkdtempDisposableSync"));
            open = (ref("node:fs.open"));
            openSync = (ref("node:fs.openSync"));
            openAsBlob = (ref("node:fs.openAsBlob"));
            readdir = (ref("node:fs.readdir"));
            readdirSync = (ref("node:fs.readdirSync"));
            read = (ref("node:fs.read"));
            readSync = (ref("node:fs.readSync"));
            readv = (ref("node:fs.readv"));
            readvSync = (ref("node:fs.readvSync"));
            readFile = (ref("node:fs.readFile"));
            readFileSync = (ref("node:fs.readFileSync"));
            readlink = (ref("node:fs.readlink"));
            readlinkSync = (ref("node:fs.readlinkSync"));
            realpath = (ref("node:fs.realpath"));
            realpathSync = (ref("node:fs.realpathSync"));
            rename = (ref("node:fs.rename"));
            renameSync = (ref("node:fs.renameSync"));
            rm = (ref("node:fs.rm"));
            rmSync = (ref("node:fs.rmSync"));
            rmdir = (ref("node:fs.rmdir"));
            rmdirSync = (ref("node:fs.rmdirSync"));
            stat = (ref("node:fs.stat"));
            statfs = (ref("node:fs.statfs"));
            statSync = (ref("node:fs.statSync"));
            statfsSync = (ref("node:fs.statfsSync"));
            symlink = (ref("node:fs.symlink"));
            symlinkSync = (ref("node:fs.symlinkSync"));
            truncate = (ref("node:fs.truncate"));
            truncateSync = (ref("node:fs.truncateSync"));
            unwatchFile = (ref("node:fs.unwatchFile"));
            unlink = (ref("node:fs.unlink"));
            unlinkSync = (ref("node:fs.unlinkSync"));
            utimes = (ref("node:fs.utimes"));
            utimesSync = (ref("node:fs.utimesSync"));
            watch = (ref("node:fs.watch"));
            watchFile = (ref("node:fs.watchFile"));
            writeFile = (ref("node:fs.writeFile"));
            writeFileSync = (ref("node:fs.writeFileSync"));
            write = (ref("node:fs.write"));
            writeSync = (ref("node:fs.writeSync"));
            writev = (ref("node:fs.writev"));
            writevSync = (ref("node:fs.writevSync"));
            Dirent = (ref("node:fs.Dirent"));
            Stats = (ref("node:fs.Stats"));
            ReadStream = (accessor(get = "node:fs.default.ReadStream.<get>", set = "node:fs.default.ReadStream.<set>"), attrs = enumerable | configurable);
            WriteStream = (accessor(get = "node:fs.default.WriteStream.<get>", set = "node:fs.default.WriteStream.<set>"), attrs = enumerable | configurable);
            FileReadStream = (accessor(get = "node:fs.default.FileReadStream.<get>", set = "node:fs.default.FileReadStream.<set>"), attrs = enumerable | configurable);
            FileWriteStream = (accessor(get = "node:fs.default.FileWriteStream.<get>", set = "node:fs.default.FileWriteStream.<set>"), attrs = enumerable | configurable);
            Utf8Stream = (accessor(get = "node:fs.default.Utf8Stream.<get>"), attrs = enumerable | configurable);
            _toUnixTimestamp = (ref("node:fs._toUnixTimestamp"));
            Dir = (ref("node:fs.Dir"));
            opendir = (ref("node:fs.opendir"));
            opendirSync = (ref("node:fs.opendirSync"));
            constants = (ref("node:fs.constants"), attrs = enumerable);
            promises = (accessor(get = "node:fs.default.promises.<get>"), attrs = enumerable | configurable);
        }
    }
}

crate::realm_decl! {
    namespace NM_FS_CONSTANTS = "fs.constants" {
        cell = "node:fs.constants";
        members = {
            UV_FS_SYMLINK_DIR = (1.0);
            UV_FS_SYMLINK_JUNCTION = (2.0);
            O_RDONLY = (0.0);
            O_WRONLY = (1.0);
            O_RDWR = (2.0);
            UV_DIRENT_UNKNOWN = (0.0);
            UV_DIRENT_FILE = (1.0);
            UV_DIRENT_DIR = (2.0);
            UV_DIRENT_LINK = (3.0);
            UV_DIRENT_FIFO = (4.0);
            UV_DIRENT_SOCKET = (5.0);
            UV_DIRENT_CHAR = (6.0);
            UV_DIRENT_BLOCK = (7.0);
            S_IFMT = (61440.0);
            S_IFREG = (32768.0);
            S_IFDIR = (16384.0);
            S_IFCHR = (8192.0);
            S_IFBLK = (24576.0);
            S_IFIFO = (4096.0);
            S_IFLNK = (40960.0);
            S_IFSOCK = (49152.0);
            O_CREAT = (64.0);
            O_EXCL = (128.0);
            UV_FS_O_FILEMAP = (0.0);
            O_NOCTTY = (256.0);
            O_TRUNC = (512.0);
            O_APPEND = (1024.0);
            O_DIRECTORY = (65536.0);
            O_NOATIME = (262144.0) if cfg(target_os = "linux");
            O_SYMLINK = (2097152.0) if cfg(target_os = "macos");
            O_NOFOLLOW = (131072.0);
            O_SYNC = (1052672.0);
            O_DSYNC = (4096.0);
            O_DIRECT = (16384.0) if cfg(target_os = "linux");
            O_NONBLOCK = (2048.0);
            S_IRWXU = (448.0);
            S_IRUSR = (256.0);
            S_IWUSR = (128.0);
            S_IXUSR = (64.0);
            S_IRWXG = (56.0);
            S_IRGRP = (32.0);
            S_IWGRP = (16.0);
            S_IXGRP = (8.0);
            S_IRWXO = (7.0);
            S_IROTH = (4.0);
            S_IWOTH = (2.0);
            S_IXOTH = (1.0);
            F_OK = (0.0);
            R_OK = (4.0);
            W_OK = (2.0);
            X_OK = (1.0);
            UV_FS_COPYFILE_EXCL = (1.0);
            COPYFILE_EXCL = (1.0);
            UV_FS_COPYFILE_FICLONE = (2.0);
            COPYFILE_FICLONE = (2.0);
            UV_FS_COPYFILE_FICLONE_FORCE = (4.0);
            COPYFILE_FICLONE_FORCE = (4.0);
        }
    }
}
