import * as path from "node:path";

// #1728: a bare drive letter is a drive-relative ref → "C:." (the drive's cwd).
console.log("norm bare-drive:", path.win32.normalize("C:"));
console.log("norm bare-drive-lower:", path.win32.normalize("c:"));

// #1728: a trailing separator the input carried is preserved.
console.log("norm dot-sep:", path.win32.normalize(".\\"));
console.log("norm longpath-root:", path.win32.normalize("\\\\?\\C:\\"));
console.log("norm drive-trailing:", path.win32.normalize("C:\\foo\\"));

// Regression guards — existing win32.normalize behavior must hold.
console.log("norm drive-abs:", path.win32.normalize("C:\\foo\\bar\\..\\baz"));
console.log("norm unc-content:", path.win32.normalize("\\\\server\\share\\foo\\..\\bar"));
console.log("norm drive-rel:", path.win32.normalize("C:foo"));

// #1728: win32.basename of a UNC root returns the share segment.
console.log("base unc-root:", path.win32.basename("\\\\server\\share\\"));
console.log("base unc-file:", path.win32.basename("\\\\server\\share\\file"));
console.log("base drive:", path.win32.basename("C:\\foo\\bar\\baz.txt"));

// #1728: toNamespacedPath keeps the UNC root's trailing separator.
console.log("ns unc-root:", path.win32.toNamespacedPath("\\\\server\\share"));
console.log("ns unc-file:", path.win32.toNamespacedPath("\\\\server\\share\\file"));
console.log("ns drive:", path.win32.toNamespacedPath("C:\\foo"));
console.log("ns prefixed:", path.win32.toNamespacedPath("\\\\?\\C:\\already"));
