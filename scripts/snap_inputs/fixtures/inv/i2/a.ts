// Values with native state, taken in an init: a stream, a typed view, a regexp.
export const err = process.stderr;
export const bytes = new Uint8Array(16);
export const re = /(ab+)c/g;
re.exec("xabbbc");
