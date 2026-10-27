; banks_sgram: banks.inc for one board (MAPPER 3). The code sits where that board keeps its fixed ROM.
MAPPER   = 3
CODE     = $C000
CODE_END = $10000
        include "banks.inc"
