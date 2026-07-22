/* kernel/src/arch/asm.S - Assembly entry point and context switching */
.section .text
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

.global context_switch
context_switch:
    /* a0 = old context (Context*), a1 = new context (Context*) */
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

.global uservec
uservec:
    /* User trap entry - save user registers, switch to kernel stack */
    csrrw sp, sscratch, sp
    sd ra, 0(sp)
    sd gp, 8(sp)
    sd tp, 16(sp)
    sd t0, 24(sp)
    sd t1, 32(sp)
    sd t2, 40(sp)
    sd t3, 48(sp)
    sd t4, 56(sp)
    sd t5, 64(sp)
    sd t6, 72(sp)
    sd s0, 80(sp)
    sd s1, 88(sp)
    sd s2, 96(sp)
    sd s3, 104(sp)
    sd s4, 112(sp)
    sd s5, 120(sp)
    sd s6, 128(sp)
    sd s7, 136(sp)
    sd s8, 144(sp)
    sd s9, 152(sp)
    sd s10, 160(sp)
    sd s11, 168(sp)
    sd a0, 176(sp)
    sd a1, 184(sp)
    sd a2, 192(sp)
    sd a3, 200(sp)
    sd a4, 208(sp)
    sd a5, 216(sp)
    sd a6, 224(sp)
    sd a7, 232(sp)
    csrr t0, sstatus
    sd t0, 240(sp)
    csrr t0, sepc
    sd t0, 248(sp)
    csrr t0, sscratch
    sd t0, 256(sp)
    j trap_handler

.global userret
userret:
    /* Return to user mode - restore user registers */
    ld ra, 0(sp)
    ld gp, 8(sp)
    ld tp, 16(sp)
    ld t0, 24(sp)
    ld t1, 32(sp)
    ld t2, 40(sp)
    ld t3, 48(sp)
    ld t4, 56(sp)
    ld t5, 64(sp)
    ld t6, 72(sp)
    ld s0, 80(sp)
    ld s1, 88(sp)
    ld s2, 96(sp)
    ld s3, 104(sp)
    ld s4, 112(sp)
    ld s5, 120(sp)
    ld s6, 128(sp)
    ld s7, 136(sp)
    ld s8, 144(sp)
    ld s9, 152(sp)
    ld s10, 160(sp)
    ld s11, 168(sp)
    ld a0, 176(sp)
    ld a1, 184(sp)
    ld a2, 192(sp)
    ld a3, 200(sp)
    ld a4, 208(sp)
    ld a5, 216(sp)
    ld a6, 224(sp)
    ld a7, 232(sp)
    ld t0, 240(sp)
    csrw sstatus, t0
    ld t0, 248(sp)
    csrw sepc, t0
    ld t0, 256(sp)
    csrw sscratch, t0
    sret