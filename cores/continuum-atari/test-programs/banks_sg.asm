; banks_sg: banks.inc for one board (MAPPER 1). The code sits where that board keeps its fixed ROM.
MAPPER   = 1
CODE     = $C000
CODE_END = $10000
        include "banks.inc"
