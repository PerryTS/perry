A compiled module's evaluation record is named by the order in which modules
first evaluate, not by the address the linker gave its guard global. The guard
is now the low byte of an eight-byte global and the runtime keeps the record
number in the bytes above it, so the registry key text (hashed and compared on
every module entry) is the same in every link of the same program.
