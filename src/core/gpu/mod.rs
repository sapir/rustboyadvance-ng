use std::fmt;

use super::arm7tdmi::{Addr, Bus};
use super::palette::{PixelFormat, Rgb15};
use super::*;

use crate::bitfield::Bit;
use crate::num::FromPrimitive;

const VRAM_ADDR: Addr = 0x0600_0000;

#[derive(Debug, Primitive, Clone, Copy)]
enum BGMode {
    BGMode0 = 0,
    BGMode1 = 1,
    BGMode2 = 2,
    BGMode3 = 3,
    BGMode4 = 4,
    BGMode5 = 5,
}

impl From<u16> for BGMode {
    fn from(v: u16) -> BGMode {
        BGMode::from_u16(v).unwrap()
    }
}

bitfield! {
    pub struct DisplayControl(u16);
    impl Debug;
    u16;
    into BGMode, mode, set_mode: 2, 0;
    display_frame, set_display_frame: 4, 4;
    hblank_interval_free, _: 5;
    obj_character_vram_mapping, _: 6;
    forst_vblank, _: 7;
    disp_bg0, _ : 8;
    disp_bg1, _ : 9;
    disp_bg2, _ : 10;
    disp_bg3, _ : 11;
    disp_obj, _ : 12;
    disp_window0, _ : 13;
    disp_window1, _ : 14;
    disp_obj_window, _ : 15;
}

impl DisplayControl {
    fn disp_bg(&self, bg: usize) -> bool {
        self.0.bit(8 + bg)
    }
}

bitfield! {
    pub struct DisplayStatus(u16);
    impl Debug;
    u16;
    get_vblank, set_vblank: 0;
    get_hblank, set_hblank: 1;
    get_vcount, set_vcount: 2;
    vblank_irq_enable, _ : 3;
    hblank_irq_enable, _ : 4;
    vcount_irq_enable, _ : 5;
    vcount_setting, _ : 15, 8;
}

bitfield! {
    #[derive(Copy, Clone)]
    pub struct BgControl(u16);
    impl Debug;
    u16;
    bg_priority, _: 1, 0;
    character_base_block, _: 3, 2;
    moasic, _ : 6;
    palette256, _ : 7;
    screen_base_block, _: 12, 8;
    affine_wraparound, _: 13;
    bg_size, _ : 15, 14;
}

const SCREEN_BLOCK_SIZE: u32 = 0x800;

impl BgControl {
    pub fn char_block(&self) -> Addr {
        VRAM_ADDR + (self.character_base_block() as u32) * 0x4000
    }

    pub fn screen_block(&self) -> Addr {
        VRAM_ADDR + (self.screen_base_block() as u32) * SCREEN_BLOCK_SIZE
    }

    fn size_regular(&self) -> (u32, u32) {
        match self.bg_size() {
            0b00 => (256, 256),
            0b01 => (512, 256),
            0b10 => (256, 512),
            0b11 => (512, 512),
            _ => unreachable!(),
        }
    }

    pub fn tile_format(&self) -> (u32, PixelFormat) {
        if self.palette256() {
            (2 * Gpu::TILE_SIZE, PixelFormat::BPP8)
        } else {
            (Gpu::TILE_SIZE, PixelFormat::BPP4)
        }
    }
}

#[derive(Debug, PartialEq, Copy, Clone)]
pub enum GpuState {
    HDraw = 0,
    HBlank,
    VBlank,
}
impl Default for GpuState {
    fn default() -> GpuState {
        GpuState::HDraw
    }
}
use GpuState::*;

pub struct FrameBuffer([Rgb15; 512 * 512]);

impl fmt::Debug for FrameBuffer {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "FrameBuffer: ")?;
        for i in 0..6 {
            let (r, g, b) = self.0[i].get_rgb24();
            write!(f, "#{:x}{:x}{:x}, ", r, g, b)?;
        }
        write!(f, "...")
    }
}

impl std::ops::Index<usize> for FrameBuffer {
    type Output = Rgb15;
    fn index(&self, index: usize) -> &Self::Output {
        &self.0[index]
    }
}

impl std::ops::IndexMut<usize> for FrameBuffer {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.0[index]
    }
}

#[derive(Debug)]
pub struct Gpu {
    // registers
    pub dispcnt: DisplayControl,
    pub dispstat: DisplayStatus,
    pub bgcnt: [BgControl; 4],
    pub bgvofs: [u16; 4],
    pub bghofs: [u16; 4],
    pub win0h: u16,
    pub win1h: u16,
    pub win0v: u16,
    pub win1v: u16,
    pub winin: u16,
    pub winout: u16,
    pub mosaic: u16,
    pub bldcnt: u16,
    pub bldalpha: u16,
    pub bldy: u16,

    cycles: usize,

    pub pixeldata: FrameBuffer,
    pub state: GpuState,
    pub current_scanline: usize, // VCOUNT
}

impl Gpu {
    pub const DISPLAY_WIDTH: usize = 240;
    pub const DISPLAY_HEIGHT: usize = 160;

    pub const CYCLES_PIXEL: usize = 4;
    pub const CYCLES_HDRAW: usize = 960;
    pub const CYCLES_HBLANK: usize = 272;
    pub const CYCLES_SCANLINE: usize = 1232;
    pub const CYCLES_VDRAW: usize = 197120;
    pub const CYCLES_VBLANK: usize = 83776;

    pub const TILE_SIZE: u32 = 0x20;

    pub fn new() -> Gpu {
        Gpu {
            dispcnt: DisplayControl(0x80),
            dispstat: DisplayStatus(0),
            bgcnt: [BgControl(0), BgControl(0), BgControl(0), BgControl(0)],
            bgvofs: [0; 4],
            bghofs: [0; 4],
            win0h: 0,
            win1h: 0,
            win0v: 0,
            win1v: 0,
            winin: 0,
            winout: 0,
            mosaic: 0,
            bldcnt: 0,
            bldalpha: 0,
            bldy: 0,

            state: HDraw,
            current_scanline: 0,
            cycles: 0,
            pixeldata: FrameBuffer([Rgb15::from(0); 512 * 512]),
        }
    }

    /// helper method that reads the palette index from a base address and x + y
    pub fn read_pixel_index(
        &self,
        sb: &SysBus,
        addr: Addr,
        x: u32,
        y: u32,
        format: PixelFormat,
    ) -> usize {
        let ofs = addr - VRAM_ADDR;
        match format {
            PixelFormat::BPP4 => {
                let byte = sb.vram.read_8(ofs + index2d!(x / 2, y, 4));
                if x & 1 != 0 {
                    (byte >> 4) as usize
                } else {
                    (byte & 0xf) as usize
                }
            }
            PixelFormat::BPP8 => sb.vram.read_8(ofs + index2d!(x, y, 8)) as usize,
        }
    }

    pub fn get_palette_color(&self, sb: &SysBus, index: u32, palette_index: u32) -> Rgb15 {
        sb.palette_ram
            .read_16(2 * index + 0x20 * palette_index)
            .into()
    }

    fn scanline_mode0(&mut self, bg: usize, sb: &mut SysBus) {
        let (h_ofs, v_ofs) = (self.bghofs[bg] as u32, self.bgvofs[bg] as u32);
        let tileset_base = self.bgcnt[bg].char_block() - VRAM_ADDR;
        let tilemap_base = self.bgcnt[bg].screen_block() - VRAM_ADDR;
        let (tile_size, pixel_format) = self.bgcnt[bg].tile_format();

        let (bg_width, bg_height) = self.bgcnt[bg].size_regular();

        let screen_y = self.current_scanline as u32;
        let mut screen_x = 0;

        // calculate the bg coords at the top-left corner, including wraparound
        let bg_x = (screen_x + h_ofs) % bg_width;
        let bg_y = (screen_y + v_ofs) % bg_height;

        // calculate the initial screen entry index
        // | (256,256) | (512,256) |  (256,512)  | (512,512) |
        // |-----------|-----------|-------------|-----------|
        // |           |           |     [1]     |  [2][3]   |
        // |    [0]    |  [0][1]   |     [0]     |  [0][1]   |
        // |___________|___________|_____________|___________|
        //
        let mut screen_block = match (bg_width, bg_height) {
            (256, 256) => 0,
            (512, 256) => bg_x / 256,
            (256, 512) => bg_y / 256,
            (512, 512) => index2d!(bg_x / 256, bg_y / 256, 2),
            _ => unreachable!(),
        } as u32;

        let se_row = (bg_x / 8) % 32;
        let se_column = (bg_y / 8) % 32;

        // this will be non-zero if the h-scroll lands in a middle of a tile
        let mut start_tile_x = bg_x % 8;

        for t in 0..32 {
            let map_addr = tilemap_base
                + SCREEN_BLOCK_SIZE * screen_block
                + 2 * (index2d!((se_row + t) % 32, se_column, 32) as u32);
            let entry = TileMapEntry(sb.vram.read_16(map_addr - VRAM_ADDR));
            let tile_addr = tileset_base + entry.tile_index() * tile_size;

            for tile_px in start_tile_x..=7 {
                let tile_py = (bg_y % 8) as u32;
                let index = self.read_pixel_index(
                    sb,
                    tile_addr,
                    if entry.x_flip() { 7 - tile_px } else { tile_px },
                    if entry.y_flip() { 7 - tile_py } else { tile_py },
                    pixel_format,
                );
                let palette_bank = match pixel_format {
                    PixelFormat::BPP4 => entry.palette_bank() as u32,
                    PixelFormat::BPP8 => 0u32,
                };
                let color = self.get_palette_color(sb, index as u32, palette_bank);
                if color.get_rgb24() != (0, 0, 0) {
                    self.pixeldata[index2d!(screen_x as usize, screen_y as usize, 512)] = color;
                }
                screen_x += 1;
                if (Gpu::DISPLAY_WIDTH as u32) == screen_x {
                    return;
                }
            }
            start_tile_x = 0;
            if se_row + t == 31 {
                if bg_width == 512 {
                    screen_block = screen_block ^ 1;
                }
            }
        }
    }

    fn scanline_mode3(&mut self, _bg: u32, sb: &mut SysBus) {
        let y = self.current_scanline;

        for x in 0..Self::DISPLAY_WIDTH {
            let pixel_index = index2d!(x, y, Self::DISPLAY_WIDTH);
            let pixel_ofs = 2 * (pixel_index as u32);
            self.pixeldata[index2d!(x, y, 512)] = sb.vram.read_16(pixel_ofs).into();
        }
    }

    fn scanline_mode4(&mut self, _bg: u32, sb: &mut SysBus) {
        let page_ofs: u32 = match self.dispcnt.display_frame() {
            0 => 0x0600_0000 - VRAM_ADDR,
            1 => 0x0600_a000 - VRAM_ADDR,
            _ => unreachable!(),
        };

        let y = self.current_scanline;

        for x in 0..Self::DISPLAY_WIDTH {
            let bitmap_index = index2d!(x, y, Self::DISPLAY_WIDTH);
            let bitmap_ofs = page_ofs + (bitmap_index as u32);
            let index = sb.vram.read_8(bitmap_ofs as Addr) as u32;
            self.pixeldata[index2d!(x, y, 512)] = self.get_palette_color(sb, index, 0);
        }
    }

    pub fn scanline(&mut self, sb: &mut SysBus) {
        match self.dispcnt.mode() {
            BGMode::BGMode0 => {
                for bg in (0..3).rev() {
                    if self.dispcnt.disp_bg(bg) {
                        self.scanline_mode0(bg, sb);
                    }
                }
            }
            BGMode::BGMode2 => {
                self.scanline_mode0(3, sb);
                self.scanline_mode0(2, sb);
            }
            BGMode::BGMode3 => {
                self.scanline_mode3(2, sb);
            }
            BGMode::BGMode4 => {
                self.scanline_mode4(2, sb);
            }
            _ => panic!("{:?} not supported", self.dispcnt.mode()),
        }
    }

    pub fn render(&self) -> Vec<u32> {
        let mut buffer = vec![0u32; Gpu::DISPLAY_WIDTH * Gpu::DISPLAY_WIDTH];
        for y in 0..Gpu::DISPLAY_HEIGHT {
            for x in 0..Gpu::DISPLAY_WIDTH {
                let (r, g, b) = self.pixeldata[index2d!(x as usize, y as usize, 512)].get_rgb24();
                buffer[index2d!(x, y, Gpu::DISPLAY_WIDTH)] =
                    ((r as u32) << 16) | ((g as u32) << 8) | (b as u32);
            }
        }
        buffer
    }
}

impl SyncedIoDevice for Gpu {
    fn step(&mut self, cycles: usize, sb: &mut SysBus, irqs: &mut IrqBitmask) {
        self.cycles += cycles;

        if self.dispstat.vcount_setting() != 0 {
            self.dispstat
                .set_vcount(self.dispstat.vcount_setting() == self.current_scanline as u16);
        }
        if self.dispstat.vcount_irq_enable() && self.dispstat.get_vcount() {
            irqs.set_LCD_VCounterMatch(true);;
        }

        match self.state {
            HDraw => {
                if self.cycles > Gpu::CYCLES_HDRAW {
                    self.current_scanline += 1;
                    self.cycles -= Gpu::CYCLES_HDRAW;

                    if self.current_scanline < Gpu::DISPLAY_HEIGHT {
                        self.scanline(sb);
                        // HBlank
                        self.dispstat.set_hblank(true);
                        if self.dispstat.hblank_irq_enable() {
                            irqs.set_LCD_HBlank(true);
                        };
                        self.state = HBlank;
                    } else {
                        self.scanline(sb);
                        self.dispstat.set_vblank(true);
                        if self.dispstat.vblank_irq_enable() {
                            irqs.set_LCD_VBlank(true);
                        };
                        self.state = VBlank;
                    };
                }
            }
            HBlank => {
                if self.cycles > Gpu::CYCLES_HBLANK {
                    self.cycles -= Gpu::CYCLES_HBLANK;
                    self.state = HDraw;
                    self.dispstat.set_hblank(false);
                    self.dispstat.set_vblank(false);
                }
            }
            VBlank => {
                if self.cycles > Gpu::CYCLES_VBLANK {
                    self.cycles -= Gpu::CYCLES_VBLANK;
                    self.state = HDraw;
                    self.dispstat.set_hblank(false);
                    self.dispstat.set_vblank(false);
                    self.current_scanline = 0;
                    self.scanline(sb);
                }
            }
        }
    }
}

bitfield! {
    struct TileMapEntry(u16);
    u16;
    u32, tile_index, _: 9, 0;
    x_flip, _ : 10;
    y_flip, _ : 11;
    palette_bank, _ : 15, 12;
}