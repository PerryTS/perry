`globalThis.navigator` is now an accessor like Node 26's (get only, enumerable,
configurable). Its getter builds the navigator object on the first read and
keeps it, so `LC_ALL` and `LANG` (navigator.language and languages) and the
CPU count (navigator.hardwareConcurrency) are read on first use instead of
while the realm is populated. Programs that never touch `navigator` no longer
build it at startup.
