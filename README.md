# gbemu (better name TBD)

This is a very WIP Game Boy emulator, with an Iced frontend

## Current state:
- Implemented CPU, passes Blargg's cpu-instr tests, and all SSTs (with the exception of the STOP instruction, which is unimplemented currently)
- PPU implementation, passes dmg_acid2
- No OAM DMA implemented yet
- No sound
- No controls
- No serial
- No mappers