; banks_sg9: banks.inc for one board (MAPPER 2). The code sits where that board keeps its fixed ROM.
MAPPER   = 2
CODE     = $C000
CODE_END = $10000
        include "banks.inc"
