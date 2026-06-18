# gbemu (better name TBD)

This is a very WIP Game Boy emulator, with an Slint frontend

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
- PPU implementation, passes dmg_acid2, displays Prehistorik Man
- Partially working sound
- Basic serial for test output
- MBC0 (No mapper) and MBC1 are currently implemented
