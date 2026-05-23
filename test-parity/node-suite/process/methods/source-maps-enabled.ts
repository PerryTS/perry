// process.sourceMapsEnabled (boolean getter) + process.setSourceMapsEnabled
// (function). Perry is AOT and doesn't ship a source-map resolver, so the
// getter always reads false and the setter is a no-op. Tests on shape only
// since Node's exact toggle behavior depends on --enable-source-maps.
// Regression cover for #1400.
console.log("sourceMapsEnabled typeof:", typeof process.sourceMapsEnabled);
console.log("setSourceMapsEnabled typeof:", typeof process.setSourceMapsEnabled);
console.log("setSourceMapsEnabled returns:", process.setSourceMapsEnabled(true));
console.log("setSourceMapsEnabled false ok:", process.setSourceMapsEnabled(false));
