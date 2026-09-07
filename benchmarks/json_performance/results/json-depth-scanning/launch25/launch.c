#include <stdio.h>
#include <unistd.h>

// launch <immutable executable> <argv0> [args ...]
// Replace this process; the benchmark's own CPU timer starts after exec.
int main(int argc, char **argv) {
    if (argc < 3) {
        fputs("usage: launch <executable> <argv0> [args ...]\n", stderr);
        return 64;
    }
    execv(argv[1], argv + 2);
    perror("execv");
    return 126;
}
