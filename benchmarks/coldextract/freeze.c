/* Adapted from perry-nativeattr/frozen_sampler.c: stop an owned PID, wait
 * for every thread, read smaps while frozen, then resume on every exit. */
#define _GNU_SOURCE
#include <sys/types.h>
#include <signal.h>
#include <unistd.h>
#include <fcntl.h>
#include <dirent.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
static int pid,paused;
static void resume(void){if(paused){kill(pid,SIGCONT);paused=0;}}
static int stopped(void){char path[512],line[4096];snprintf(path,sizeof(path),"/proc/%d/task",pid);DIR*d=opendir(path);if(!d)return 0;int n=0,ok=1;struct dirent*e;while((e=readdir(d))){if(e->d_name[0]=='.')continue;snprintf(path,sizeof(path),"/proc/%d/task/%s/stat",pid,e->d_name);FILE*f=fopen(path,"r");if(!f||!fgets(line,sizeof(line),f)){if(f)fclose(f);ok=0;break;}fclose(f);char*end=strrchr(line,')');if(!end||end[2]!='T'){ok=0;break;}n++;}closedir(d);return ok&&n;}
int main(int argc,char**argv){if(argc!=4)return 98;pid=atoi(argv[1]);char path[128],actual[4096];snprintf(path,sizeof(path),"/proc/%d/exe",pid);ssize_t n=readlink(path,actual,sizeof(actual)-1);if(n<0)return 75;actual[n]=0;if(strcmp(actual,argv[3]))return 98;atexit(resume);if(kill(pid,SIGSTOP))return 75;paused=1;struct timespec tiny={0,100000};int ready=0;for(int i=0;i<200;i++){if(stopped()){ready=1;break;}nanosleep(&tiny,0);}if(!ready)return 75;snprintf(path,sizeof(path),"/proc/%d/smaps",pid);int fd=open(path,O_RDONLY),out=open(argv[2],O_WRONLY|O_CREAT|O_TRUNC,0600);if(fd<0||out<0)return 75;char buf[65536];while((n=read(fd,buf,sizeof(buf)))>0){ssize_t pos=0;while(pos<n){ssize_t w=write(out,buf+pos,n-pos);if(w<=0)return 98;pos+=w;}}close(fd);close(out);resume();return 0;}
