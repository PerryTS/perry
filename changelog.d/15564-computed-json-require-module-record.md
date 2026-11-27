A computed `require()` of an absolute `.json` path uses the module record like every other require, so it is read once and keeps its identity (nextjs read middleware-manifest.json on every request).
