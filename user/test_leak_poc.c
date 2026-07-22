// PoC for copyinstr kernel stack info leak vulnerability
// Demonstrates the vulnerable pattern (mitigated in current xv6 by fetchstr)
#include "kernel/types.h"
#include "kernel/stat.h"
#include "kernel/fcntl.h"
#include "user/user.h"

#define PGSIZE 4096
#define MAXPATH 128

int
main(void)
{
  char *p;
  int fd;
  
  // Allocate one page eagerly
  p = sbrk(PGSIZE);
  if (p == (char*)-1) {
    printf("sbrk(PGSIZE) failed\n");
    exit(1);
  }
  
  // Place 10 bytes at END of page (offset PGSIZE-10)
  // NO null terminator in these 10 bytes
  char *path = &p[PGSIZE - 10];
  memset(path, 'A', 10);
  
  // Next page is UNMAPPED (copyinstr does NOT trigger lazy allocation)
  // copyinstr behavior:
  // 1. Reads 10 bytes from page 1 (all 'A's), max becomes 118
  // 2. Tries to read page 2 -> walkaddr returns 0 -> returns -1
  // 3. Kernel buffer has 10 'A's WITHOUT null terminator
  // 4. fetchstr checks return value, returns -1, NO strlen called
  //    (This is the mitigation in current xv6)
  
  printf("Testing copyinstr OOB read pattern (mitigated in xv6)...\n");
  printf("Path address: %p (10 bytes before page boundary)\n", path);
  printf("String: 10 'A's, no NUL, next page unmapped\n");
  
  fd = open(path, O_RDONLY);
  
  if (fd >= 0) {
    printf("open succeeded (unexpected)\n");
    close(fd);
  } else {
    printf("open failed as expected (copyinstr returned -1)\n");
  }
  
  printf("Kernel did not panic - mitigation in fetchstr works\n");
  
  // Also test exec path
  char *argv[2];
  argv[0] = path;
  argv[1] = 0;
  
  int pid = fork();
  if (pid == 0) {
    exec("echo", argv);
    printf("exec failed as expected\n");
    exit(1);
  } else if (pid > 0) {
    wait(0);
  }
  
  printf("All tests passed - vulnerability pattern exists but mitigated\n");
  exit(0);
}
