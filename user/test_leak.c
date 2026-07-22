// Test for kernel stack info leak via fetchstr + copyinstr bug
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
  
  // Fill first page with 'A's (127 bytes, no NUL)
  // We need the string to cross page boundary within MAXPATH (128) bytes
  // So put 127 'A's at end of page 1
  memset(p, 'A', PGSIZE - 128);  // Fill most of page
  // Last 127 bytes of page 1
  for (int i = 0; i < 127; i++) {
    p[PGSIZE - 128 + i] = 'A';
  }
  // No NUL in page 1
  
  // Lazily allocate second page
  char *p2 = sbrklazy(PGSIZE);
  if (p2 == (char*)-1) {
    printf("sbrklazy(PGSIZE) failed\n");
    exit(1);
  }
  
  // Put NUL at start of page 2
  p2[0] = '\0';
  
  // Path points to the 127 'A's at end of page 1
  char *path = &p[PGSIZE - 128];
  
  printf("Testing kernel stack info leak...\n");
  printf("Path address: %p\n", path);
  printf("Path content: 127 'A's then crosses to lazy page with NUL\n");
  
  // This should trigger the bug:
  // copyinstr copies 127 bytes to kernel buf[128], no NUL
  // Then tries to read page 2, fails, returns -1
  // fetchstr calls strlen(buf) on 127-byte non-NUL-terminated buffer
  // strlen reads past buf into kernel stack!
  fd = open(path, O_RDONLY);
  
  if (fd >= 0) {
    printf("open succeeded (unexpected)\n");
    close(fd);
  } else {
    printf("open failed as expected\n");
  }
  
  // Also test with exec
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
  
  printf("Test completed\n");
  exit(0);
}