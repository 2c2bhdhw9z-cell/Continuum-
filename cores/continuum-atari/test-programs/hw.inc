; The 7800's registers, by the names Atari's 7800 Software Guide gives them.
; cAtari's own test programs (GPL-3.0-only), assembled with dasm.

; TIA
INPTCTRL = $01
INPT0    = $08
INPT1    = $09
INPT2    = $0A
INPT3    = $0B
INPT4    = $0C
INPT5    = $0D
AUDC0    = $15
AUDC1    = $16
AUDF0    = $17
AUDF1    = $18
AUDV0    = $19
AUDV1    = $1A

; MARIA
BACKGRND = $20
P0C1     = $21
P0C2     = $22
P0C3     = $23
WSYNC    = $24
P1C1     = $25
P1C2     = $26
P1C3     = $27
MSTAT    = $28
P2C1     = $29
P2C2     = $2A
P2C3     = $2B
DPPH     = $2C
P3C1     = $2D
P3C2     = $2E
P3C3     = $2F
DPPL     = $30
P4C1     = $31
P4C2     = $32
P4C3     = $33
CHARBASE = $34
P5C1     = $35
P5C2     = $36
P5C3     = $37
OFFSET   = $38
P6C1     = $39
P6C2     = $3A
P6C3     = $3B
CTRL     = $3C
P7C1     = $3D
P7C2     = $3E
P7C3     = $3F

; RIOT
SWCHA    = $280
CTLSWA   = $281
SWCHB    = $282
CTLSWB   = $283
INTIM    = $284
TIM1T    = $294
TIM8T    = $295
TIM64T   = $296
T1024T   = $297

; MARIA CTRL values
DMA_ON   = $40        ; DMA on, one-byte characters, black border, transparency, 160A/B
DMA_OFF  = $60
