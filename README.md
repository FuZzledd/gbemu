# gbemu (better name TBD)

This is a very WIP Game Boy emulator.

Current state:
- Implemented CPU, passes most of Blargg's cpu-instr tests, and SSTs (with the exception of the STOP instruction, which is unimplemented currently)
- Basic PPU implementation, seemingly functional, only BG layer rendering for now.
- No OAM DMA implemented yet
- No sound
- No controls