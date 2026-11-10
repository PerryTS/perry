// Build with wasi-sdk 34 (Preview 1, not a component):
// $WASI_SDK_PATH/bin/clang --target=wasm32-wasip1 -O2 -Wl,--strip-all libc-command.c -o libc-command.wasm
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/uio.h>
#include <unistd.h>
int main(int argc, char **argv) {
    printf("libc args: %d %s\n", argc, argc > 1 ? argv[1] : "missing");
    printf("libc env: %s\n", getenv("PERRY_TEST") ? getenv("PERRY_TEST") : "missing");
    FILE *file = fopen("/sandbox/created.txt", "w+");
    if (!file) { perror("fopen"); return 10; }
    if (fwrite("abcdef", 1, 6, file) != 6 || fflush(file) || fseek(file, 2, SEEK_SET)) return 11;
    char text[5] = {0};
    if (fread(text, 1, 4, file) != 4) return 12;
    printf("libc seek/read: %s %ld\n", text, ftell(file));
    if (fclose(file)) return 13;
    fflush(stdout);
    struct iovec vectors[2] = {{"scatter", 7}, {"/gather\n", 8}};
    if (writev(1, vectors, 2) != 15) return 14;
    if (unlink("/sandbox/created.txt")) return 15;
    return 0;
}
