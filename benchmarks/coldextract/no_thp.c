#define _GNU_SOURCE
#include <sys/prctl.h>
#include <unistd.h>
#include <stdio.h>
int main(int argc,char **argv) {
  if(argc<2 || prctl(PR_SET_THP_DISABLE,1,0,0,0) || prctl(PR_GET_THP_DISABLE,0,0,0,0)!=1) {perror("disable THP");return 96;}
  execvp(argv[1],argv+1);perror("exec");return 96;
}
