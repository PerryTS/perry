#include <sys/prctl.h>
#include <unistd.h>
#include <stdio.h>
#include <stdlib.h>
int main(int argc, char **argv) {
  if (argc < 2) return 2;
  if (prctl(PR_SET_THP_DISABLE, 1, 0, 0, 0) != 0 ||
      prctl(PR_GET_THP_DISABLE, 0, 0, 0, 0) != 1) {
    perror("disable THP"); return 2;
  }
  setenv("MIMALLOC_ALLOW_THP", "0", 1);
  execvp(argv[1], argv + 1); perror("exec"); return 2;
}
