`pbkdf2Sync`, `argon2Sync` and `argon2` imported by name from `node:crypto`
returned `undefined` (or never called back) once any module in the program
imported `node:module`. The dynamic crypto dispatcher now handles all three.
