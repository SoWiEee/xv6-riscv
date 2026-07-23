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

/* uservec: trampoline entry from user mode */
uservec:
    /* swap user/kernel page tables */
    csrrw t0, satp, a0
    sfence.vma
    
    /* get trapframe pointer from sscratch */
    csrr a0, sscratch
    
    /* save user registers to trapframe */
    sd ra, 40(a0)
    sd sp, 48(a0)
    sd gp, 56(a0)
    sd tp, 64(a0)
    sd t0, 72(a0)
    sd t1, 80(a0)
    sd t2, 88(a0)
    sd s0, 96(a0)
    sd s1, 104(a0)
    sd a0, 112(a0)
    sd a1, 120(a0)
    sd a2, 128(a0)
    sd a3, 136(a0)
    sd a4, 144(a0)
    sd a5, 152(a0)
    sd a6, 160(a0)
    sd a7, 168(a0)
    sd s2, 176(a0)
    sd s3, 184(a0)
    sd s4, 192(a0)
    sd s5, 200(a0)
    sd s6, 208(a0)
    sd s7, 216(a0)
    sd s8, 224(a0)
    sd s9, 232(a0)
    sd s10, 240(a0)
    sd s11, 248(a0)
    sd t3, 256(a0)
    sd t4, 264(a0)
    sd t5, 272(a0)
    sd t6, 280(a0)
    
    /* load kernel registers from trapframe */
    ld sp, 8(a0)      /* kernel_sp */
    ld tp, 24(a0)     /* kernel_hartid */
    ld t0, 16(a0)     /* kernel_trap */
    
    /* jump to usertrap */
    jr t0

/* userret: return to user mode */
userret:
    /* get trapframe pointer from sscratch */
    csrr a0, sscratch
    
    /* set up sstatus for user mode */
    csrr t0, sstatus
    li t1, ~0x100     /* clear SPP (bit 8) */
    and t0, t0, t1
    li t1, 0x20       /* set SPIE (bit 5) */
    or t0, t0, t1
    csrw sstatus, t0
    
    /* save kernel_satp and epc to temp registers BEFORE restoring a0 */
    ld t0, 24(a0)     /* epc */
    csrw sepc, t0
    ld t1, 0(a0)      /* kernel_satp (full satp value) */
    
    /* restore user registers */
    ld ra, 40(a0)
    ld sp, 48(a0)
    ld gp, 56(a0)
    ld tp, 64(a0)
    /* t0, t1 hold epc, satp, need to restore t2 from trapframe */
    ld t2, 88(a0)
    ld s0, 96(a0)
    ld s1, 104(a0)
    ld a0, 112(a0)
    ld a1, 120(a0)
    ld a2, 128(a0)
    ld a3, 136(a0)
    ld a4, 144(a0)
    ld a5, 152(a0)
    ld a6, 160(a0)
    ld a7, 168(a0)
    ld s2, 176(a0)
    ld s3, 184(a0)
    ld s4, 192(a0)
    ld s5, 200(a0)
    ld s6, 208(a0)
    ld s7, 216(a0)
    ld s8, 224(a0)
    ld s9, 232(a0)
    ld s10, 240(a0)
    ld s11, 248(a0)
    ld t3, 256(a0)
    ld t4, 264(a0)
    ld t5, 272(a0)
    ld t6, 280(a0)
    /* restore t0, t1 from saved values */
    /* t0 already has epc (in sepc), t1 has satp */
    
    /* switch to user page table */
    csrrw t0, satp, t1
    sfence.vma
    
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