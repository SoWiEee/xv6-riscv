// user/forkbench.c
//
// C-xv6 counterpart of user/src/bin/forkbench.rs. N iterations of fork()+wait()
// with the child exiting immediately. Exercises uvmcopy, proc allocation, the
// scheduler round-trip, and uvmfree. Difference two N values (T(2N)-T(N)) to
// cancel fixed overhead.
#include "kernel/types.h"
#include "user/user.h"

int
main(int argc, char *argv[])
{
  int n = argc > 1 ? atoi(argv[1]) : 1000;

  int t0 = uptime();
  for (int i = 0; i < n; i++) {
    int pid = fork();
    if (pid < 0) {
      printf("forkbench: fork failed\n");
      exit(1);
    }
    if (pid == 0)
      exit(0);
    wait(0);
  }
  int t1 = uptime();

  printf("FORKBENCH n=%d ticks=%d\n", n, t1 - t0);
  exit(0);
}
