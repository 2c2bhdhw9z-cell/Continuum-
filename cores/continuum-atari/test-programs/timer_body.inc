; timer_body.inc: the RIOT timer checks, the same instructions in the 7800 and 2600 versions so
; every read comes the same number of cycles after its write. Results go to values+0 to +13.
TIMINT  = $285

        ; 1: TIM1T = 20, read at once
        lda #20
        sta TIM1T
        lda INTIM
        sta values+0
        ; 2: TIM1T = 20, read 10 cycles later
        lda #20
        sta TIM1T
        nop
        nop
        nop
        nop
        nop
        lda INTIM
        sta values+1
        ; 3: TIM8T = 3, read 13 and 30 cycles later
        lda #3
        sta TIM8T
        nop
        nop
        nop
        nop
        nop
        lda INTIM
        sta values+2
        nop
        nop
        nop
        nop
        nop
        nop
        lda INTIM
        sta values+3
        ; 4: TIM64T = 1, the flag before and after it runs out
        lda #1
        sta TIM64T
        lda TIMINT
        sta values+4
        ldx #30
w4:     dex
        bne w4
        lda TIMINT
        sta values+5
        lda INTIM           ; after the end: counting once a cycle (wrapped)
        sta values+6
        lda INTIM           ; this read started the prescale again
        sta values+7
        ; 5: T1024T = 2, read twice
        lda #2
        sta T1024T
        lda INTIM
        sta values+8
        ldx #200
w5:     dex
        bne w5
        lda INTIM
        sta values+9
        ; 6: TIM1T = 2, past its end, read three times in a row
        lda #2
        sta TIM1T
        nop
        nop
        nop
        nop
        lda INTIM
        sta values+10
        lda INTIM
        sta values+11
        lda TIMINT
        sta values+12
        ; 7: the interrupt-enable addresses read the same count
        lda #40
        sta $29C            ; TIM1T with the interrupt on
        lda $28C            ; INTIM with the interrupt on
        sta values+13
