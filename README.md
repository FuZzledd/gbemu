# gbemu (better name TBD)

This is a very WIP Game Boy emulator, with an Iced frontend

# Building
Requires Rust Nightly.

Just run `cargo build`

## Controls

| Keyboard | GameBoy |
| --- | --- |
| <kbd>W</kbd><kbd>A</kbd><kbd>S</kbd><kbd>D</kbd> | D-Pad |
| <kbd>J</kbd> | A |
| <kbd>K</kbd> | B |
| <kbd>Backspace</kbd> | Select |
| <kbd>Enter</kbd> | Start |

## Current state:
- Implemented CPU, passes Blargg's cpu-instr tests, and all SSTs (with the exception of the STOP instruction, which is unimplemented currently)
- PPU implementation, passes dmg_acid2
- No sound
- No serial
- No mappers
