//! Item-level native namespace declarations for `path` (A2-DESIGN §5): each item is the
//! module's keys in Node's order and what each one is; the realm producer reads
//! the source text, the vtable static it declares carries the keys to `Object.keys`.

crate::realm_decl! {
    namespace NM_PATH_POSIX_DEFAULT = "path.posix.default" {
        cell = "node:path.posix";
        births = [Array, Error, Function, Object];
        members = {
            resolve = (ref("node:path/posix.resolve"));
            normalize = (ref("node:path/posix.normalize"));
            isAbsolute = (ref("node:path/posix.isAbsolute"));
            join = (ref("node:path/posix.join"));
            relative = (ref("node:path/posix.relative"));
            toNamespacedPath = (ref("node:path/posix._makeLong"));
            dirname = (ref("node:path/posix.dirname"));
            basename = (ref("node:path/posix.basename"));
            extname = (ref("node:path/posix.extname"));
            format = (ref("node:path/posix.format"));
            parse = (ref("node:path/posix.parse"));
            matchesGlob = (ref("node:path/posix.matchesGlob"));
            sep = ("/");
            delimiter = (":");
            win32 = (ref("node:path.win32"));
            posix = (ref("node:path.posix"));
            _makeLong = (ref("node:path/posix._makeLong"));
        }
    }
}

crate::realm_decl! {
    namespace NM_PATH_WIN32_DEFAULT = "path.win32.default" {
        cell = "node:path.win32";
        births = [Array, Error, Function, Object];
        members = {
            resolve = (ref("node:path/win32.resolve"));
            normalize = (ref("node:path/win32.normalize"));
            isAbsolute = (ref("node:path/win32.isAbsolute"));
            join = (ref("node:path/win32.join"));
            relative = (ref("node:path/win32.relative"));
            toNamespacedPath = (ref("node:path/win32._makeLong"));
            dirname = (ref("node:path/win32.dirname"));
            basename = (ref("node:path/win32.basename"));
            extname = (ref("node:path/win32.extname"));
            format = (ref("node:path/win32.format"));
            parse = (ref("node:path/win32.parse"));
            matchesGlob = (ref("node:path/win32.matchesGlob"));
            sep = ("\\");
            delimiter = (";");
            win32 = (ref("node:path.win32"));
            posix = (ref("node:path.posix"));
            _makeLong = (ref("node:path/win32._makeLong"));
        }
    }
}

crate::realm_decl! {
    namespace NM_PATH_DEFAULT = "path.default" {
        cell = "node:path.default";
        members = {
            resolve = (ref("node:path.resolve"));
            normalize = (ref("node:path.normalize"));
            isAbsolute = (ref("node:path.isAbsolute"));
            join = (ref("node:path.join"));
            relative = (ref("node:path.relative"));
            toNamespacedPath = (ref("node:path._makeLong"));
            dirname = (ref("node:path.dirname"));
            basename = (ref("node:path.basename"));
            extname = (ref("node:path.extname"));
            format = (ref("node:path.format"));
            parse = (ref("node:path.parse"));
            matchesGlob = (ref("node:path.matchesGlob"));
            sep = ("/");
            delimiter = (":");
            win32 = (ref("node:path.win32"));
            posix = (ref("node:path.posix"));
            _makeLong = (ref("node:path._makeLong"));
        }
    }
}
