// Test program for lazy sbrk + copyinstr bug
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
  
  // First, grow heap lazily to near TRAPFRAME
  // TRAPFRAME is at 0x3fffffe000, so we need to grow a lot
  // But we can't grow that much due to physical memory limits
  // Instead, let's test the copyinstr behavior with a string crossing page boundary
  
  // Allocate one page eagerly
  p = sbrk(PGSIZE);
  if (p == (char*)-1) {
    printf("sbrk(PGSIZE) failed\n");
    exit(1);
  }
  
  // Write a string that crosses the page boundary
  // Fill the page with 'A's, no null terminator
  memset(p, 'A', PGSIZE - 1);
  p[PGSIZE - 1] = 'B'; // Last byte of page
  
  // Now lazily allocate the next page
  char *p2 = sbrklazy(PGSIZE);
  if (p2 == (char*)-1) {
    printf("sbrklazy(PGSIZE) failed\n");
    exit(1);
  }
  
  // Put null terminator in the second (lazily allocated) page
  p2[0] = '\0';
  
  // Now try to open a file with this string as path
  // The string crosses a page boundary where the second page is lazily allocated
  // sys_open -> argstr -> fetchstr -> copyinstr
  // copyinstr should fail because the second page is not mapped yet
  
  // Create a dummy file first
  fd = open("testfile", O_CREATE | O_WRONLY);
  if (fd >= 0) {
    write(fd, "hello", 5);
    close(fd);
  }
  
  // Try to open using our cross-page string
  // This should fail with copyinstr returning -1
  fd = open(p, O_RDONLY);
  if (fd >= 0) {
    printf("open succeeded unexpectedly!\n");
    close(fd);
  } else {
    printf("open failed as expected (copyinstr bug)\n");
  }
  
  // Test 2: Try exec with argv crossing page boundary
  char *argv[2];
  argv[0] = p;  // Points to string crossing page boundary
  argv[1] = 0;
  
  // This should also fail in fetchstr
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