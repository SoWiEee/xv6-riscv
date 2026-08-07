// user/syscallbench.c
//
// C-xv6 counterpart of user/src/bin/syscallbench.rs. Tight getpid() loop to
// isolate syscall entry/exit cost. Prints the SYSCALLBENCH marker + guest
// uptime-tick delta. Use two N values and difference them (T(2N)-T(N)) to
// cancel fixed overhead. Kept byte-for-byte equivalent in structure to the
// Rust version so the comparison is fair.
#include "kernel/types.h"
#include "user/user.h"

int
main(int argc, char *argv[])
{
  int n = argc > 1 ? atoi(argv[1]) : 1000000;

  int t0 = uptime();
  volatile long acc = 0;
  for (int i = 0; i < n; i++)
    acc += getpid();
  int t1 = uptime();

  printf("SYSCALLBENCH n=%d ticks=%d acc=%d\n", n, t1 - t0, (int)acc);
  exit(0);
}
