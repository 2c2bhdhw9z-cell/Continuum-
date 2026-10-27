; banks_abs: banks.inc for one board (MAPPER 4). The code sits where that board keeps its fixed ROM.
MAPPER   = 4
CODE     = $8000
CODE_END = $10000
        include "banks.inc"
