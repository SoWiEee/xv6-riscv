// Test for kernel stack info leak via fetchstr + copyinstr bug
// Trigger: string with NO null within MAXPATH, next page unmapped
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
  
  // Fill last MAXPATH bytes of page with 'A's (NO NUL)
  // Path points to PGSIZE - MAXPATH
  char *path = &p[PGSIZE - MAXPATH];
  memset(path, 'A', MAXPATH);  // Exactly MAXPATH 'A's, no NUL
  
  // Do NOT allocate second page - it's unmapped
  // copyinstr will read MAXPATH bytes from first page (all 'A's)
  // Then try to read second page -> walkaddr returns 0 -> copyinstr returns -1
  // But buf has MAXPATH 'A's with NO NUL
  // fetchstr calls strlen(buf) -> reads past buf into kernel stack!
  
  printf("Testing kernel stack info leak (no NUL, next page unmapped)...\n");
  printf("Path address: %p\n", path);
  printf("Path content: %d 'A's, no NUL, next page unmapped\n", MAXPATH);
  
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
