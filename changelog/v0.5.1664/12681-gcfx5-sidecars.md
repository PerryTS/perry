`perry-gc-effects sidecars <lib-dir>` writes a `.gc-effects` effects table
beside every prebuilt runtime archive, keyed by archive size and SHA-256, for
the runtime alone and for runtime plus stdlib. Symbols defined more than once in
a combination take the weaker of their definitions. Release legs, packaging
(tarballs, npm, deb, Homebrew) and the workspace release recipes produce and
ship the sidecars. Nothing reads them yet.
