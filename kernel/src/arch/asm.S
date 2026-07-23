/* kernel/src/arch/asm.S */
.section .text
.global swtch
.global uservec
.global userret
.global kernelvec

/* void swtch(struct context *old, struct context *new) */
swtch:
    sd ra, 0(a0)
    sd sp, 8(a0)
    sd s0, 16(a0)
    sd s1, 24(a0)
    sd s2, 32(a0)
    sd s3, 40(a0)
    sd s4, 48(a0)
    sd s5, 56(a0)
    sd s6, 64(a0)
    sd s7, 72(a0)
    sd s8, 80(a0)
    sd s9, 88(a0)
    sd s10, 96(a0)
    sd s11, 104(a0)
    ld ra, 0(a1)
    ld sp, 8(a1)
    ld s0, 16(a1)
    ld s1, 24(a1)
    ld s2, 32(a1)
    ld s3, 40(a1)
    ld s4, 48(a1)
    ld s5, 56(a1)
    ld s6, 64(a1)
    ld s7, 72(a1)
    ld s8, 80(a1)
    ld s9, 88(a1)
    ld s10, 96(a1)
    ld s11, 104(a1)
    ret

/*
 * Trampoline: uservec / userret.
 *
 * This code is mapped at the same virtual address (TRAMPOLINE) in the kernel
 * page table and in every user page table, so it keeps executing across the
 * satp switch. The per-process TrapFrame is mapped at the fixed virtual
 * address TRAPFRAME (= TRAMPOLINE - PGSIZE) in each user page table.
 *
 * TrapFrame field offsets (repr(C), see arch/trap.rs):
 *   0 kernel_satp   8 kernel_sp   16 kernel_trap   24 epc   32 kernel_hartid
 *  40 ra  48 sp  56 gp  64 tp  72 t0  80 t1  88 t2
 *  96 s0 104 s1 112 s2 120 s3 128 s4 136 s5 144 s6 152 s7 160 s8 168 s9
 * 176 s10 184 s11 192 a0 200 a1 208 a2 216 a3 224 a4 232 a5 240 a6 248 a7
 * 256 t3 264 t4 272 t5 280 t6
 */
.equ TRAPFRAME, 0xffffffffffffe000

/* uservec: trap entry from user mode (stvec points here, user pt still active) */
uservec:
    /* stash user a0 in sscratch, then point a0 at the trapframe */
    csrw sscratch, a0
    li a0, TRAPFRAME

    /* save user registers (a0 handled via sscratch below) */
    sd ra, 40(a0)
    sd sp, 48(a0)
    sd gp, 56(a0)
    sd tp, 64(a0)
    sd t0, 72(a0)
    sd t1, 80(a0)
    sd t2, 88(a0)
    sd s0, 96(a0)
    sd s1, 104(a0)
    sd s2, 112(a0)
    sd s3, 120(a0)
    sd s4, 128(a0)
    sd s5, 136(a0)
    sd s6, 144(a0)
    sd s7, 152(a0)
    sd s8, 160(a0)
    sd s9, 168(a0)
    sd s10, 176(a0)
    sd s11, 184(a0)
    sd a1, 200(a0)
    sd a2, 208(a0)
    sd a3, 216(a0)
    sd a4, 224(a0)
    sd a5, 232(a0)
    sd a6, 240(a0)
    sd a7, 248(a0)
    sd t3, 256(a0)
    sd t4, 264(a0)
    sd t5, 272(a0)
    sd t6, 280(a0)

    /* save the user a0 that we parked in sscratch */
    csrr t0, sscratch
    sd t0, 192(a0)

    /* load kernel state saved for us by usertrapret */
    ld sp, 8(a0)       /* kernel_sp */
    ld tp, 32(a0)      /* kernel_hartid */
    ld t0, 16(a0)      /* kernel_trap = usertrap */
    ld t1, 0(a0)       /* kernel_satp */

    /* switch to the kernel page table */
    csrw satp, t1
    sfence.vma zero, zero

    /* jump to usertrap (does not return here) */
    jr t0

/*
 * userret: return to user mode.
 * Called by usertrapret as userret(user_satp) with a0 = user satp.
 * sstatus/sepc are already set by usertrapret.
 */
userret:
    /* switch to the user page table (a0 = user satp) */
    csrw satp, a0
    sfence.vma zero, zero

    /* a0 -> trapframe (fixed VA, mapped in the user page table) */
    li a0, TRAPFRAME

    /* restore user registers (a0 restored last) */
    ld ra, 40(a0)
    ld sp, 48(a0)
    ld gp, 56(a0)
    ld tp, 64(a0)
    ld t0, 72(a0)
    ld t1, 80(a0)
    ld t2, 88(a0)
    ld s0, 96(a0)
    ld s1, 104(a0)
    ld s2, 112(a0)
    ld s3, 120(a0)
    ld s4, 128(a0)
    ld s5, 136(a0)
    ld s6, 144(a0)
    ld s7, 152(a0)
    ld s8, 160(a0)
    ld s9, 168(a0)
    ld s10, 176(a0)
    ld s11, 184(a0)
    ld a1, 200(a0)
    ld a2, 208(a0)
    ld a3, 216(a0)
    ld a4, 224(a0)
    ld a5, 232(a0)
    ld a6, 240(a0)
    ld a7, 248(a0)
    ld t3, 256(a0)
    ld t4, 264(a0)
    ld t5, 272(a0)
    ld t6, 280(a0)
    ld a0, 192(a0)

    /* return to user mode */
    sret

/* kernelvec: trap entry from kernel mode */
kernelvec:
    /* save kernel registers, call kerneltrap, restore */
    csrr t0, sstatus
    csrr t1, sepc
    csrr t2, scause
    /* ... save registers to current process's trapframe ... */
    call kerneltrap
    /* ... restore ... */
    sret

.global arch_init_init
arch_init_init:
    /* Save return address */
    addi sp, sp, -16
    sd ra, 8(sp)
    /* Call Rust init function directly with jal */
    call init
    /* Restore and return */
    ld ra, 8(sp)
    addi sp, sp, 16
    ret