#define _GNU_SOURCE
#include <sys/wait.h>
#include <sys/resource.h>
#include <time.h>
#include <unistd.h>
#include <stdio.h>
#include <stdlib.h>
int main(int argc,char**argv){if(argc<3)return 99;struct timespec a,b;struct rusage r;int status;clock_gettime(CLOCK_MONOTONIC,&a);pid_t p=fork();if(p==0){execvp(argv[2],argv+2);_exit(127);}if(p<0)return 99;if(wait4(p,&status,0,&r)<0)return 99;clock_gettime(CLOCK_MONOTONIC,&b);FILE*f=fopen(argv[1],"w");if(!f)return 99;fprintf(f,"%.9f %.9f %.9f %ld\n",b.tv_sec-a.tv_sec+(b.tv_nsec-a.tv_nsec)/1e9,r.ru_utime.tv_sec+r.ru_utime.tv_usec/1e6,r.ru_stime.tv_sec+r.ru_stime.tv_usec/1e6,r.ru_maxrss);fclose(f);return WIFEXITED(status)?WEXITSTATUS(status):128+WTERMSIG(status);}
