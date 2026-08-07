// user/nop.c
//
// Minimal exec target for execbench: does nothing and exits.
#include "kernel/types.h"
#include "user/user.h"

int
main(int argc, char *argv[])
{
  exit(0);
}
