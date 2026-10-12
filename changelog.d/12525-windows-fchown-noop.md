On Windows, `fs.fchown` and `fs.fchownSync` now match Node/libuv by succeeding
after argument validation even when the descriptor is closed. Unix behavior is
unchanged.
