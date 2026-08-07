// user/execbench.c
//
// C-xv6 counterpart of user/src/bin/execbench.rs. N iterations of
// fork()+exec("nop")+wait(). Subtract forkbench to isolate exec cost.
#include "kernel/types.h"
#include "user/user.h"

int
main(int argc, char *argv[])
{
  int n = argc > 1 ? atoi(argv[1]) : 1000;
  char *xargv[] = { "nop", 0 };

  int t0 = uptime();
  for (int i = 0; i < n; i++) {
    int pid = fork();
    if (pid < 0) {
      printf("execbench: fork failed\n");
      exit(1);
    }
    if (pid == 0) {
      exec("nop", xargv);
      exit(1); // only reached if exec failed
    }
    wait(0);
  }
  int t1 = uptime();

  printf("EXECBENCH n=%d ticks=%d\n", n, t1 - t0);
  exit(0);
}
