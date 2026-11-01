### Fixed

- An npm-installed perry can link programs that import node:http, ws, net, zlib, sharp and the other well-known native modules. Their extension archives never shipped in the npm host packages; they now ship in one package per host, `@perryts/perry-ext-<triple>` (macOS and Linux glibc), which perry downloads from the GitHub release, verifies and installs into `~/.perry/ext` the first time a compile needs one (once per perry version; no network access otherwise). `PERRY_NO_AUTO_FETCH=1` turns the download off and the compile then names `npm install -g @perryts/perry-ext-<triple>@<version>` and `perry target add --ext`. An ext package npm-installed beside perry is found first.

### Changed

- The triples that have a target or ext package are one list, `npm/packaged-triples.json`, used by the publish scripts and compiled into perry. A cross target without a package (for example `--target windows` from Linux) keeps the generic missing-library error instead of naming a package that does not exist. `perry target add` refuses such a triple up front. `PERRY_RELEASE_MIRROR` points `perry target add` and the ext download at a directory or https mirror of the release assets.
