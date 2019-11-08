use super::arm7tdmi::{Addr, Bus};
use super::ioregs::consts::*;
use super::sysbus::SysBus;
use super::{Interrupt, IrqBitmask, SyncedIoDevice};

use num::FromPrimitive;

#[derive(Debug)]
enum DmaTransferType {
    Xfer16bit,
    Xfer32bit,
}

#[derive(Debug, Default)]
pub struct DmaChannel {
    id: usize,

    pub src: u32,
    pub dst: u32,
    pub wc: u32,
    pub ctrl: DmaChannelCtrl,

    cycles: usize,
    start_cycles: usize,
}

impl DmaChannel {
    fn new(id: usize) -> DmaChannel {
        if id > 3 {
            panic!("invalid dma id {}", id);
        }
        DmaChannel {
            id: id,
            ..DmaChannel::default()
        }
    }

    fn get_irq(&self) -> Interrupt {
        Interrupt::from_usize(self.id + 8).unwrap()
    }

    fn update(&mut self, cycles: usize, sb: &mut SysBus, irqs: &mut IrqBitmask) {
        if self.ctrl.is_enabled() {
            if self.start_cycles == 0 && cycles > self.start_cycles + 2 {
                self.xfer(sb, irqs)
            }
        }
    }

    fn xfer(&mut self, sb: &mut SysBus, irqs: &mut IrqBitmask) {
        let word_size = if self.ctrl.is_32bit() { 4 } else { 2 };
        let dst_rld = self.dst;
        for word in 0..self.wc {
            if word_size == 4 {
                let w = sb.read_32(self.src);
                sb.write_32(self.dst, w)
            } else {
                let hw = sb.read_16(self.src);
                println!("src {:x} dst {:x}", self.src, self.dst);
                sb.write_16(self.dst, hw)
            }
            match self.ctrl.src_adj() {
                /* Increment */ 0 => self.src += word_size,
                /* Decrement */ 1 => self.src -= word_size,
                /* Fixed */ 2 => {}
                _ => panic!("forbidden DMA source address adjustment"),
            }
            match self.ctrl.dst_adj() {
                /* Increment[+Reload] */ 0 | 3 => self.dst += word_size,
                /* Decrement */ 1 => self.dst -= word_size,
                /* Fixed */ 2 => {}
                _ => panic!("forbidden DMA dest address adjustment"),
            }
        }
        if self.ctrl.is_triggering_irq() {
            irqs.add_irq(self.get_irq());
        }
        if self.ctrl.repeat() {
            self.start_cycles = self.cycles;
            if
            /* reload */
            3 == self.ctrl.dst_adj() {
                self.dst = dst_rld;
            }
        } else {
            self.ctrl.set_enabled(false);
        }
    }
}

#[derive(Debug)]
pub struct DmaController {
    pub channels: [DmaChannel; 4],
    cycles: usize,
}

impl SyncedIoDevice for DmaController {
    fn step(&mut self, cycles: usize, sb: &mut SysBus, irqs: &mut IrqBitmask) {
        self.cycles += cycles;
        for ch in 0..4 {
            self.channels[ch].update(self.cycles, sb, irqs);
        }
    }
}

impl DmaController {
    pub fn new() -> DmaController {
        DmaController {
            channels: [
                DmaChannel::new(0),
                DmaChannel::new(1),
                DmaChannel::new(2),
                DmaChannel::new(3),
            ],
            cycles: 0,
        }
    }

    pub fn write_src_low(&mut self, ch: usize, low: u16) {
        let src = self.channels[ch].src;
        self.channels[ch].src = (src & 0xffff0000) | (low as u32);
    }

    pub fn write_src_high(&mut self, ch: usize, high: u16) {
        let src = self.channels[ch].src;
        let high = high as u32;
        self.channels[ch].src = (src & 0xffff) | (high << 16);
    }

    pub fn write_dst_low(&mut self, ch: usize, low: u16) {
        let dst = self.channels[ch].dst;
        self.channels[ch].dst = (dst & 0xffff0000) | (low as u32);
    }

    pub fn write_dst_high(&mut self, ch: usize, high: u16) {
        let dst = self.channels[ch].dst;
        let high = high as u32;
        self.channels[ch].dst = (dst & 0xffff) | (high << 16);
    }

    pub fn write_word_count(&mut self, ch: usize, value: u16) {
        let value = value as u32;
        self.channels[ch].wc = value;
    }

    pub fn notify_vblank(&mut self, sb: &mut SysBus, irqs: &mut IrqBitmask) {
        for ch in 0..4 {
            if !self.channels[ch].ctrl.is_enabled() {
                continue;
            }
            if self.channels[ch].ctrl.timing() == 1 {
                self.channels[ch].xfer(sb, irqs)
            }
        }
    }
    pub fn notify_hblank(&mut self, sb: &mut SysBus, irqs: &mut IrqBitmask) {
        for ch in 0..4 {
            if !self.channels[ch].ctrl.is_enabled() {
                continue;
            }
            if self.channels[ch].ctrl.timing() == 2 {
                self.channels[ch].xfer(sb, irqs)
            }
        }
    }

    pub fn write_dma_ctrl(&mut self, ch: usize, value: u16) {
        let ctrl = DmaChannelCtrl(value);
        if ctrl.is_enabled() {
            self.channels[ch].start_cycles = self.cycles;
        }
        self.channels[ch].ctrl = ctrl;
    }
}

bitfield! {
    #[derive(Default)]
    pub struct DmaChannelCtrl(u16);
    impl Debug;
    u16;
    dst_adj, _ : 6, 5;
    src_adj, _ : 8, 7;
    repeat, _ : 9;
    is_32bit, _: 10;
    timing, _: 13, 12;
    is_triggering_irq, _: 14;
    is_enabled, set_enabled: 15;
}
