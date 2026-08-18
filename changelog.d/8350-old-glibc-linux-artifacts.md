Linux GNU compiler, runtime, stdlib, and extension artifacts now build against
glibc 2.31 while the GTK4 archive continues to build separately on Ubuntu
24.04. Native packages now run on Ubuntu 20.04+, Debian 11+, RHEL 9, and Amazon
Linux 2023; older glibc systems continue to use the static musl fallback.

Release CI also checks the compiler's imported GLIBC symbol versions and runs a
compiled TypeScript program inside the old-glibc build environment. Debian
packages declare the matching `libc6 (>= 2.31)` requirement.
