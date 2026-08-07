// user/fsbench.c
//
// C-xv6 counterpart of user/src/bin/fsbench.rs. Create a file and write N KiB
// in 1024-byte writes to exercise the block layer. Difference two N values.
#include "kernel/types.h"
#include "kernel/fcntl.h"
#include "user/user.h"

int
main(int argc, char *argv[])
{
  int n = argc > 1 ? atoi(argv[1]) : 300;
  static char buf[1024];
  for (int i = 0; i < 1024; i++)
    buf[i] = 0x61;

  unlink("fsbench.tmp");
  int t0 = uptime();
  int fd = open("fsbench.tmp", O_CREATE | O_RDWR);
  if (fd < 0) {
    printf("fsbench: open failed\n");
    exit(1);
  }
  for (int i = 0; i < n; i++) {
    if (write(fd, buf, 1024) != 1024) {
      printf("fsbench: write failed\n");
      exit(1);
    }
  }
  close(fd);
  int t1 = uptime();
  unlink("fsbench.tmp");

  printf("FSBENCH n=%d ticks=%d\n", n, t1 - t0);
  exit(0);
}
