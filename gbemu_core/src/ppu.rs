#![allow(unused)]

use core::{
    iter::Cloned,
    mem,
    ops::{BitAnd, BitOr, Deref, DerefMut, Index, IndexMut, Not, Shl, Shr},
    slice,
};
use std::process::Output;

use crate::context::{Context, InterruptRegister, Io, Memory, Memory8K, MemoryBus};
use crate::{bit_getters, get_bit, set_bit};
use array_deque::{ArrayDeque, StackArrayDeque};
use better_default::Default;
use bytemuck::TransparentWrapper;
use paste::paste;
use strum::FromRepr;
use tap::Pipe;
use tracing::instrument;

#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Lcdc(u8);

impl DerefMut for Lcdc {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Deref for Lcdc {
    type Target = u8;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl Lcdc {
    bit_getters!(enable, 7);

    fn window_tile_map(&self) -> TileMapArea {
        TileMapArea::from_repr(get_bit(self.0, 6) as u8).unwrap()
    }

    fn set_window_tile_map(&mut self, map: TileMapArea) {
        set_bit(&mut self.0, 6, map as u8 != 0);
    }

    bit_getters!(window_enable, 5);

    fn tile_data_mapping(&self) -> TileDataMapping {
        TileDataMapping::from_repr(get_bit(self.0, 4) as u8).unwrap()
    }

    fn set_tile_data_mapping(&mut self, map: TileDataMapping) {
        set_bit(&mut self.0, 4, map as u8 != 0);
    }

    fn bg_tile_map(&self) -> TileMapArea {
        TileMapArea::from_repr(get_bit(self.0, 3) as u8).unwrap()
    }

    fn set_bg_tile_map(&mut self, map: TileMapArea) {
        set_bit(&mut self.0, 3, map as u8 != 0);
    }

    fn obj_size(&self) -> ObjSize {
        ObjSize::from_repr(get_bit(self.0, 2) as u8).unwrap()
    }

    fn set_obj_size(&mut self, map: ObjSize) {
        set_bit(&mut self.0, 2, map as u8 != 0);
    }

    bit_getters!(obj_enable, 1);

    bit_getters!(bg_window_enable, 0);
}

#[derive(Debug, Copy, Clone, Default)]
#[repr(transparent)]
struct Stat(u8);

impl DerefMut for Stat {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Deref for Stat {
    type Target = u8;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Stat {
    bit_getters!(lyc_select, 6);
    bit_getters!(mode_2, 5);
    bit_getters!(mode_1, 4);
    bit_getters!(mode_0, 3);
    bit_getters!(lyc_equal, 2);
    fn ppu_mode(&self) -> Mode {
        Mode::from_repr(self.0 as usize & 0b11).unwrap()
    }

    fn set_ppu_mode(&mut self, mode: Mode) {
        self.0 = (self.0 & !0b11) | (mode as u8 & 0b11);
    }
}

#[derive(Debug, Default)]
pub struct LCDRegisters {
    lcdc: Lcdc,
    stat: Stat,
    scy: u8,
    scx: u8,
    pub ly: u8,
    lyc: u8,
    dma: u8,
    bgp: u8,
    obp: [u8; 2],
    wy: u8,
    wx: u8,
    start_dma: bool,
    pub(crate) dma_source_address: u8,
    pub(crate) dma_counter: Option<u8>,
}

impl LCDRegisters {
    pub(crate) fn read(&self, address: u8) -> u8 {
        match address {
            0x40 => *self.lcdc,
            0x41 => *self.stat,
            0x42 => self.scy,
            0x43 => self.scx,
            0x44 => self.ly,
            0x45 => self.lyc,
            0x46 => self.dma,
            0x47 => self.bgp,
            0x48 => self.obp[0],
            0x49 => self.obp[1],
            0x4A => self.wy,
            0x4B => self.wx,
            _ => unreachable!(),
        }
    }

    pub(crate) fn write(&mut self, address: u8, value: u8) {
        match address {
            0x40 => *self.lcdc = value,
            0x41 => *self.stat = (*self.stat & 0b111) | (value & !0b111),
            0x42 => self.scy = value,
            0x43 => self.scx = value,
            0x44 => {}
            0x45 => self.lyc = value,
            0x46 => {
                self.initiate_oam_dma(value);
            }
            0x47 => self.bgp = value,
            0x48 => self.obp[0] = value,
            0x49 => self.obp[1] = value,
            0x4A => self.wy = value,
            0x4B => self.wx = value,
            _ => unreachable!(),
        }
    }
    fn initiate_oam_dma(&mut self, value: u8) {
        self.dma_source_address = value;
        self.dma_counter = Some(0);
    }
}

#[repr(transparent)]
#[derive(Default)]
pub struct Vram(#[default([0; 1024 * 8])] Memory8K);

impl DerefMut for Vram {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Deref for Vram {
    type Target = Memory8K;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Copy, Clone, Debug, FromRepr)]
#[repr(u8)]
enum TileMapArea {
    Zero,
    One,
}

#[derive(Copy, Clone, Debug, FromRepr)]
#[repr(u8)]
enum ObjSize {
    Square,
    Tall,
}

#[derive(Copy, Clone, Debug, FromRepr)]
#[repr(u8)]
enum TileDataMapping {
    Zero = 1,
    One = 0,
}

const VRAM_BASE_ADDRESS: usize = 0x8000;

impl Vram {
    pub fn tile_data(&self) -> &[u8] {
        &self.0[0x8000 - VRAM_BASE_ADDRESS..=0x97FF - VRAM_BASE_ADDRESS]
    }

    fn tile_map(&self, index: TileMapArea) -> &[u8] {
        &self.0[Self::get_tile_map_range(index)]
    }
    fn tile_map_mut(&mut self, index: TileMapArea) -> &mut [u8] {
        &mut self.0[Self::get_tile_map_range(index)]
    }

    fn get_tile_map_range(index: TileMapArea) -> std::ops::RangeInclusive<usize> {
        match index {
            TileMapArea::Zero => 0x9800 - VRAM_BASE_ADDRESS..=0x9BFF - VRAM_BASE_ADDRESS,
            TileMapArea::One => 0x9C00 - VRAM_BASE_ADDRESS..=0x9FFF - VRAM_BASE_ADDRESS,
        }
    }

    fn window_tile_map(&self, ctx: &Context<MemoryBus>) -> &[u8] {
        self.tile_map(ctx.memory.io.lcd.lcdc.window_tile_map())
    }
    fn window_tile_map_mut(&mut self, ctx: &mut Context<MemoryBus>) -> &mut [u8] {
        self.tile_map_mut(ctx.memory.io.lcd.lcdc.window_tile_map())
    }

    fn sprite_tile_data(&self, sprite_size: ObjSize, tile_no: u8) -> &[u8] {
        let tile_data = &self.0[0x8000 - VRAM_BASE_ADDRESS..=0x8FFF - VRAM_BASE_ADDRESS];
        let (tiles, []) = tile_data.as_chunks::<16>() else {
            unreachable!()
        };
        match sprite_size {
            ObjSize::Square => &tiles[tile_no as usize],
            ObjSize::Tall => {
                let (tiles, []) = tile_data.as_chunks::<32>() else {
                    unreachable!()
                };
                &tiles[(tile_no / 2) as usize]
            }
        }
    }

    fn bg_tile_data(&self, mapping: TileDataMapping, tile_no: u8) -> &[u8; 16] {
        match mapping {
            TileDataMapping::Zero => {
                let tile_data = &self.0[0x8000 - VRAM_BASE_ADDRESS..=0x8FFF - VRAM_BASE_ADDRESS];
                let (tiles, []) = tile_data.as_chunks::<16>() else {
                    unreachable!()
                };
                &tiles[tile_no as usize]
            }
            TileDataMapping::One => {
                let tile_data = &self.0[0x8800 - VRAM_BASE_ADDRESS..=0x97FF - VRAM_BASE_ADDRESS];
                let (tiles, []) = tile_data.as_chunks::<16>() else {
                    unreachable!()
                };
                &tiles[tile_no.wrapping_add(128) as usize]
            }
        }
    }
    fn bg_tile_data_mut(&mut self, mapping: TileDataMapping, tile_no: u8) -> &mut [u8; 16] {
        match mapping {
            TileDataMapping::Zero => {
                let tile_data =
                    &mut self.0[0x8000 - VRAM_BASE_ADDRESS..=0x8FFF - VRAM_BASE_ADDRESS];
                let (tiles, []) = tile_data.as_chunks_mut::<16>() else {
                    unreachable!()
                };
                &mut tiles[tile_no as usize]
            }
            TileDataMapping::One => {
                let tile_data =
                    &mut self.0[0x8800 - VRAM_BASE_ADDRESS..=0x97FF - VRAM_BASE_ADDRESS];
                let (tiles, []) = tile_data.as_chunks_mut::<16>() else {
                    unreachable!()
                };
                &mut tiles[tile_no.wrapping_add(128) as usize]
            }
        }
    }
}
#[repr(transparent)]
#[derive(Clone, Copy, Default)]
pub(crate) struct Oam(#[default([0;0xFEA0 - 0xFE00])] [u8; 0xFEA0 - 0xFE00]);

impl DerefMut for Oam {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Deref for Oam {
    type Target = [u8; 0xFEA0 - 0xFE00];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Oam {
    fn oam_entries(&self) -> &[OamEntry] {
        let (oam_entries, []) = self.0.as_chunks::<4>() else {
            unreachable!()
        };
        OamEntry::wrap_slice(oam_entries)
    }

    fn oam_entries_mut(&mut self) -> &[OamEntry] {
        let (oam_entries, []) = self.0.as_chunks_mut::<4>() else {
            unreachable!()
        };
        OamEntry::wrap_slice_mut(oam_entries)
    }
}

#[derive(Debug, Clone, Copy, Default, TransparentWrapper)]
#[repr(transparent)]
struct OamAttributes(u8);

impl DerefMut for OamAttributes {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Deref for OamAttributes {
    type Target = u8;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl OamAttributes {
    bit_getters!(priority, 7);
    bit_getters!(y_flip, 6);
    bit_getters!(x_flip, 5);
    bit_getters!(dmg_palette, 4);
    bit_getters!(bank, 3);
    fn cgb_palette(&self) -> u8 {
        self.0 & 0b111
    }
    fn set_cgb_palette(&mut self, value: u8) {
        assert!(value <= 0b111);
        self.0 = (self.0 & !0b111) | (value & 0b111);
    }
}

#[derive(Debug, Clone, Copy, Default, TransparentWrapper)]
#[repr(transparent)]
struct OamEntry([u8; 4]);

impl DerefMut for OamEntry {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
impl Deref for OamEntry {
    type Target = [u8; 4];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl OamEntry {
    fn y(&self) -> u8 {
        self.0[0]
    }
    fn y_mut(&mut self) -> &mut u8 {
        &mut self.0[0]
    }
    fn x(&self) -> u8 {
        self.0[1]
    }
    fn x_mut(&mut self) -> &mut u8 {
        &mut self.0[1]
    }

    fn tile_index(&self) -> u8 {
        self.0[2]
    }
    fn tile_index_mut(&mut self) -> &mut u8 {
        &mut self.0[2]
    }

    fn attributes(&self) -> &OamAttributes {
        OamAttributes::wrap_ref(&self.0[3])
    }
    fn attributes_mut(&mut self) -> &mut OamAttributes {
        OamAttributes::wrap_mut(&mut self.0[3])
    }
}

#[derive(Copy, Clone, Debug, FromRepr, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    OamScan = 2,
    PixelTransfer = 3,

    HBlank = 0,
    VBlank = 1,
}

struct SpritePixel {
    palette_index: u8,
    source: PixelSource,
    palette: bool,
    priority: bool,
}

#[derive(Debug, Clone, Copy, FromRepr, Default, Hash, PartialEq, Eq)]
#[repr(u8)]
pub enum Pixel {
    #[default]
    White = 0,
    LightGray,
    DarkGrey,
    Black,
}

#[derive(Debug, Clone, Copy, FromRepr)]
enum PixelSource {
    S0 = 0,
    S1,
    S2,
    S3,
    S4,
    S5,
    S6,
    S7,
    S8,
    S9,
}

// Reference for PPU details: https://www.youtube.com/watch?v=HyzD8pNlpwI&t=1760s
#[derive(Default)]
pub struct PPU {
    pub cycle_counter: u32,
    pub current_mode: Mode,
    obj_buffer: StackArrayDeque<OamEntry, 10>,
    #[default(StackArrayDeque::new())]
    oam_copy: StackArrayDeque<OamEntry, 40>,
    bg_fifo: StackArrayDeque<u8, 8>,
    sprite_fifo: StackArrayDeque<SpritePixel, 8>,
    bg_fetcher_state: BgFetcherState,
    sprite_fetcher_state: SpriteFetcherState,
    screen_x: u8,
    scx_counter: u8,
    #[default(StackArrayDeque::new())]
    sprites_to_fetch: StackArrayDeque<(usize, OamEntry), 10>,
    fetching_sprite: Option<OamEntry>,
    oam_index: usize,
    #[default([Default::default(); 160*144])]
    pub screen: [Pixel; 160 * 144],
    stat_interrupt_line: bool,
    first_tile_fetch: bool,
    sprites_fetched: bool,
    window_y_condition: bool,
    in_window: bool,
    window_row: u8,
}

#[derive(Default, Clone, Copy, Debug)]
struct BgFetcherState {
    tile_no: Option<u8>,
    data_low: Option<u8>,
    data_high: Option<u8>,
    tile_line: u8,
}
impl BgFetcherState {
    fn clear(&mut self) {
        *self = Self::default()
    }
}

#[derive(Default, Clone, Copy, Debug)]
struct SpriteFetcherState {
    tile_no: Option<u8>,
    data_low: Option<u8>,
    data_high: Option<u8>,
    tile_line: u8,
    oam_index: usize,
    priority: bool,
    y_flip: bool,
    x_flip: bool,
    palette: bool,
}
impl SpriteFetcherState {
    fn clear(&mut self) {
        *self = Self::default()
    }
}

impl PPU {
    #[instrument(skip_all)]
    pub fn tick(&mut self, ctx: &mut Context<MemoryBus>) {
        match self.current_mode {
            Mode::OamScan => {
                self.oam_scan(ctx);
                self.cycle_counter += 1;
            }
            Mode::PixelTransfer => {
                self.pixel_transfer(ctx);
                self.cycle_counter += 1;
            }
            Mode::HBlank => {
                if self.cycle_counter == 455 {
                    if ctx.memory.io.lcd.ly == 143 {
                        self.current_mode = Mode::VBlank;
                    } else {
                        self.current_mode = Mode::OamScan;
                    }
                    ctx.memory.io.lcd.ly += 1;
                    self.cycle_counter = 0;
                } else {
                    self.cycle_counter += 1;
                }
            }
            Mode::VBlank => {
                if self.cycle_counter == 0 && ctx.memory.io.lcd.ly == 144 {
                    self.window_y_condition = false;
                    self.window_row = 255;
                    ctx.memory
                        .io_mut()
                        .interrupt_flag_mut()
                        .schedule_interrupt(crate::context::InterruptType::VBlank);
                }
                if self.cycle_counter == 455 {
                    if ctx.memory.io.lcd.ly == 153 {
                        self.current_mode = Mode::OamScan;
                        self.cycle_counter = 0;
                        ctx.memory.io.lcd.ly = 0;
                    } else {
                        ctx.memory.io.lcd.ly += 1;
                        self.cycle_counter = 0;
                        self.screen = [Default::default(); 160 * 144];
                    }
                } else {
                    self.cycle_counter += 1;
                }
            }
        };
        ctx.memory
            .io
            .lcd
            .stat
            .set_lyc_equal(ctx.memory.io.lcd.ly == ctx.memory.io.lcd.lyc);
        ctx.memory.io.lcd.stat.set_ppu_mode(self.current_mode);
        let stat_line = (ctx.memory.io.lcd.stat.lyc_select() && ctx.memory.io.lcd.stat.lyc_equal())
            || (ctx.memory.io.lcd.stat.mode_2() && self.current_mode == Mode::OamScan)
            || (ctx.memory.io.lcd.stat.mode_1() && self.current_mode == Mode::VBlank)
            || (ctx.memory.io.lcd.stat.mode_0() && self.current_mode == Mode::HBlank);
        if !self.stat_interrupt_line && stat_line {
            ctx.memory
                .io
                .interrupt
                .schedule_interrupt(crate::context::InterruptType::LCD);
        }
        self.stat_interrupt_line = stat_line;
    }

    fn oam_scan(&mut self, ctx: &mut Context<MemoryBus>) {
        match self.cycle_counter {
            0 => {
                self.obj_buffer.clear();
                self.oam_copy = ctx.memory.oam.oam_entries().iter().cloned().collect();
            }
            x if x % 2 == 0 => {
                //Stall
            }
            x => {
                if !self.obj_buffer.is_full()
                    && let Some(next_obj) = self.oam_copy.pop_front()
                    && next_obj.x() != 0
                    && object_on_scanline(
                        next_obj.y(),
                        ctx.memory.io.lcd.ly,
                        ctx.memory.io.lcd.lcdc.obj_size(),
                    )
                {
                    self.obj_buffer.push_back(next_obj);
                }
            }
        }
        if self.cycle_counter == 79 {
            self.current_mode = Mode::PixelTransfer
        }
    }

    fn pixel_transfer(&mut self, ctx: &mut Context<MemoryBus>) {
        if self.cycle_counter == 80 {
            self.screen_x = 0u8.wrapping_sub(8);
            self.scx_counter = ctx.memory.io.lcd.scx % 8;
            self.first_tile_fetch = true;
            if ctx.memory.io.lcd.wy <= ctx.memory.io.lcd.ly {
                self.window_y_condition = true;
            }
        }
        // if !ctx.memory.io.lcd.lcdc.window_enable() {
        //     self.window_y_condition = false;
        // }

        if self.window_y_condition
            && self.screen_x.wrapping_add(7) == ctx.memory.io.lcd.wx
            && ctx.memory.io.lcd.lcdc.window_enable()
            && !self.in_window
        {
            self.in_window = true;
            self.bg_fetcher_state.clear();
            self.bg_fifo.clear();
            self.first_tile_fetch = true;
            self.window_row = self.window_row.wrapping_add(1);
        }
        if self.in_window
            && self.bg_fifo.is_empty()
            && (self.screen_x.wrapping_add(7) < ctx.memory.io.lcd.wx
                || !ctx.memory.io.lcd.lcdc.window_enable())
        {
            self.in_window = false;
            self.bg_fetcher_state.clear();
            self.bg_fifo.clear();
            self.first_tile_fetch = true;
        }

        if !self.sprites_fetched {
            self.sprites_to_fetch = self
                .obj_buffer
                .iter()
                .cloned()
                .enumerate()
                .filter(|(index, entry)| entry.x().wrapping_sub(8) == self.screen_x)
                .collect();
            self.sprites_fetched = true
        };
        if !ctx.memory.io.lcd.lcdc.obj_enable() {
            self.sprite_fetcher_state.clear();
            self.sprites_to_fetch.clear();
        }
        if self.cycle_counter % 2 == 1 {
            if !self.sprites_to_fetch.is_empty() && ctx.memory.io.lcd.lcdc.obj_enable() {
                self.sprite_fetch(ctx);
            } else {
                self.bg_fetch(ctx);
            }
        }
        if !self.bg_fifo.is_empty() && self.sprites_to_fetch.is_empty() {
            if self.scx_counter > 0 {
                self.bg_fifo.pop_front();
                self.scx_counter -= 1;
            } else {
                let bg_palette_index = self.bg_fifo.pop_front().unwrap();

                let colour = if let Some(sprite_pixel) = self.sprite_fifo.pop_front() {
                    let sprite_palette = ctx.memory.io.lcd.obp[sprite_pixel.palette as usize];
                    if sprite_pixel.palette_index == 0
                        || (sprite_pixel.priority && bg_palette_index != 0)
                        || !ctx.memory.io.lcd.lcdc.obj_enable()
                    {
                        if ctx.memory.io.lcd.lcdc.bg_window_enable() {
                            Pixel::from_repr(
                                (ctx.memory.io.lcd.bgp >> (bg_palette_index * 2)) & 0b11,
                            )
                            .unwrap()
                        } else {
                            Pixel::White
                        }
                    } else {
                        Pixel::from_repr(
                            (sprite_palette >> (sprite_pixel.palette_index * 2)) & 0b11,
                        )
                        .unwrap()
                    }
                } else {
                    if ctx.memory.io.lcd.lcdc.bg_window_enable() {
                        Pixel::from_repr((ctx.memory.io.lcd.bgp >> (bg_palette_index * 2)) & 0b11)
                            .unwrap()
                    } else {
                        Pixel::White
                    }
                };
                if self.screen_x < 160 {
                    self.screen[ctx.memory.io.lcd.ly as usize * 160 + self.screen_x as usize] =
                        colour;
                }
                self.screen_x = self.screen_x.wrapping_add(1);
                self.sprites_fetched = false;
                if self.screen_x == 160 {
                    self.current_mode = Mode::HBlank;
                    self.bg_fetcher_state.clear();
                    self.sprite_fetcher_state.clear();
                    self.bg_fifo.clear();
                    self.sprite_fifo.clear();
                }
            }
        }
    }

    fn bg_fetch(&mut self, ctx: &mut Context<MemoryBus>) {
        let BgFetcherState {
            tile_no,
            data_low,
            data_high,
            tile_line,
        } = &mut self.bg_fetcher_state;
        match (tile_no, data_low, data_high) {
            (None, _, _) => {
                self.fetch_bg_tile(ctx);
                if self.first_tile_fetch {
                    self.first_tile_fetch = false;
                }
            }
            (Some(tile_no), data_low @ None, _) => {
                *data_low = Some(
                    ctx.memory
                        .vram
                        .bg_tile_data(ctx.memory.io.lcd.lcdc.tile_data_mapping(), *tile_no)
                        [*tile_line as usize * 2],
                );
            }
            (Some(tile_no), Some(_), data_high @ None) => {
                *data_high = Some(
                    ctx.memory
                        .vram
                        .bg_tile_data(ctx.memory.io.lcd.lcdc.tile_data_mapping(), *tile_no)
                        [*tile_line as usize * 2 + 1],
                );
            }
            (Some(_), Some(low), Some(high)) => {}
        }
        if let (Some(low), Some(high)) = (
            self.bg_fetcher_state.data_low,
            self.bg_fetcher_state.data_high,
        ) && self.bg_fifo.is_empty()
        {
            let tile_row = u16::from_le_bytes([low, high]);
            for n in 0..8 {
                let palette_index = tile_row.extract_bits(0b1000_0000_1000_0000 >> n) as u8;
                self.bg_fifo.push_back(palette_index);
            }
            self.bg_fetcher_state.clear();
        }
    }

    fn fetch_bg_tile(&mut self, ctx: &mut Context<MemoryBus>) {
        let first_fetch_offset = if self.first_tile_fetch { 0u8 } else { 8u8 };
        let in_window = self.in_window;
        let tile_map_area = if !in_window {
            ctx.memory.io.lcd.lcdc.bg_tile_map()
        } else {
            ctx.memory.io.lcd.lcdc.window_tile_map()
        };
        let tile_x = if !in_window {
            self.screen_x
                .wrapping_add(ctx.memory.io.lcd.scx)
                .wrapping_add(first_fetch_offset)
        } else {
            self.screen_x
                .wrapping_sub(ctx.memory.io.lcd.wx.wrapping_sub(7))
                .wrapping_add(first_fetch_offset)
        };
        let tile_y = if !in_window {
            ctx.memory.io.lcd.ly.wrapping_add(ctx.memory.io.lcd.scy)
        } else {
            self.window_row
        };
        self.bg_fetcher_state.tile_line = tile_y % 8;
        let tile_map_index = (((tile_y as u16) / 8) << 5) | (tile_x as u16 / 8);
        self.bg_fetcher_state.tile_no =
            Some(ctx.memory.vram.tile_map(tile_map_area)[tile_map_index as usize]);
    }

    fn sprite_fetch(&mut self, ctx: &mut Context<MemoryBus>) {
        {
            let SpriteFetcherState {
                tile_no,
                data_low,
                data_high,
                tile_line,
                x_flip,
                y_flip,
                priority,
                palette,
                oam_index,
            } = &mut self.sprite_fetcher_state;

            match (tile_no, data_low, data_high) {
                (None, _, _) => {
                    self.fetch_sprite_tile(ctx);
                }
                (Some(tile_no), data_low @ None, _) => {
                    *data_low = Some(
                        ctx.memory
                            .vram
                            .sprite_tile_data(ctx.memory.io.lcd.lcdc.obj_size(), *tile_no)
                            .pipe(|data| {
                                if *y_flip {
                                    data[data.len() - 1 - (*tile_line as usize * 2 + 1)]
                                } else {
                                    data[*tile_line as usize * 2]
                                }
                            })
                            .pipe(|data| if *x_flip { data.reverse_bits() } else { data }),
                    );
                }
                (Some(tile_no), Some(_), data_high @ None) => {
                    *data_high = Some(
                        ctx.memory
                            .vram
                            .sprite_tile_data(ctx.memory.io.lcd.lcdc.obj_size(), *tile_no)
                            .pipe(|data| {
                                if *y_flip {
                                    data[data.len() - 1 - (*tile_line as usize * 2)]
                                } else {
                                    data[*tile_line as usize * 2 + 1]
                                }
                            })
                            .pipe(|data| if *x_flip { data.reverse_bits() } else { data }),
                    );
                }
                (Some(_), Some(low), Some(high)) => {}
            };
        }
        if let (Some(low), Some(high)) = (
            self.sprite_fetcher_state.data_low,
            self.sprite_fetcher_state.data_high,
        ) {
            let tile_row = u16::from_le_bytes([low, high]);
            self.sprite_fifo = (0..8)
                .map(|n| {
                    let palette_index = tile_row.extract_bits(0b1000_0000_1000_0000 >> n) as u8;
                    if let Some(SpritePixel {
                        palette_index: current_palette_index,
                        source,
                        palette,
                        priority,
                    }) = self.sprite_fifo.pop_front()
                    {
                        if current_palette_index == 0 {
                            SpritePixel {
                                palette_index,
                                source: PixelSource::from_repr(self.sprite_fetcher_state.oam_index)
                                    .unwrap(),
                                priority: self.sprite_fetcher_state.priority,
                                palette: self.sprite_fetcher_state.palette,
                            }
                        } else {
                            SpritePixel {
                                palette_index: current_palette_index,
                                source,
                                palette,
                                priority,
                            }
                        }
                    } else {
                        SpritePixel {
                            palette_index,
                            source: PixelSource::from_repr(self.sprite_fetcher_state.oam_index)
                                .unwrap(),
                            priority: self.sprite_fetcher_state.priority,
                            palette: self.sprite_fetcher_state.palette,
                        }
                    }
                })
                .collect();

            self.sprite_fetcher_state.clear();
            self.sprites_to_fetch.pop_front();
        }
    }

    fn fetch_sprite_tile(&mut self, ctx: &mut Context<MemoryBus>) {
        if let Some((oam_index, current_sprite)) = self.sprites_to_fetch.front() {
            self.sprite_fetcher_state.tile_line = ctx
                .memory
                .io
                .lcd
                .ly
                .wrapping_sub(current_sprite.y().wrapping_sub(16));
            self.sprite_fetcher_state.tile_no = Some(current_sprite.tile_index());
            self.sprite_fetcher_state.oam_index = *oam_index;
            self.sprite_fetcher_state.priority = current_sprite.attributes().priority();
            self.sprite_fetcher_state.y_flip = current_sprite.attributes().y_flip();
            self.sprite_fetcher_state.x_flip = current_sprite.attributes().x_flip();
            self.sprite_fetcher_state.palette = current_sprite.attributes().dmg_palette();
        }
    }
}

fn object_on_scanline(obj_y: u8, scanline_y: u8, size: ObjSize) -> bool {
    match size {
        ObjSize::Square => (obj_y..(obj_y.wrapping_add(8))),
        ObjSize::Tall => (obj_y..(obj_y.wrapping_add(16))),
    }
    .contains(&(scanline_y.wrapping_add(16)))
}
