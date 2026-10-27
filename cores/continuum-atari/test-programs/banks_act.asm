; banks_act: banks.inc for one board (MAPPER 5). The code sits where that board keeps its fixed ROM.
MAPPER   = 5
CODE     = $E000
CODE_END = $10000
        include "banks.inc"
