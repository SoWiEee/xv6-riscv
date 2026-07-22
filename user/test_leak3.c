// Test for kernel stack info leak via fetchstr + copyinstr bug
// Trigger: string crosses page boundary, NO null within MAXPATH, second page unmapped
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
  
  // Put 10 'A's at end of page 1 (offset PGSIZE - 10)
  // NO NUL in page 1
  char *path = &p[PGSIZE - 10];
  memset(path, 'A', 10);
  
  // Do NOT allocate second page - it's unmapped
  // copyinstr will:
  // 1. Read 10 bytes from page 1 (all 'A's), max becomes 118
  // 2. Try to read page 2 -> walkaddr returns 0 -> return -1
  // But buf has 10 'A's with NO NUL
  // fetchstr calls strlen(buf) -> reads past buf into kernel stack!
  
  printf("Testing kernel stack info leak (cross-page, no NUL)...\n");
  printf("Path address: %p\n", path);
  printf("Path content: 10 'A's at end of page, next page unmapped\n");
  
  // This should trigger the bug
  fd = open(path, O_RDONLY);
  
  if (fd >= 0) {
    printf("open succeeded (unexpected)\n");
    close(fd);
  } else {
    printf("open failed as expected\n");
  }
  
  printf("Test completed - if kernel didn't panic, leak happened silently\n");
  exit(0);
}
