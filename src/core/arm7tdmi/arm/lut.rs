use super::super::super::SysBus;
use super::super::Core;
use super::super::CpuAction;
use super::super::InstructionDecoder;
use super::{ArmFormat, ArmInstruction};

pub type ArmInstructionHandler = fn(&mut Core, &mut SysBus, &ArmInstruction) -> CpuAction;

impl From<ArmFormat> for ArmInstructionHandler {
    fn from(arm_fmt: ArmFormat) -> ArmInstructionHandler {
        match arm_fmt {
            ArmFormat::BX => Core::exec_arm_bx,
            ArmFormat::B_BL => Core::exec_arm_b_bl,
            ArmFormat::DP => Core::exec_arm_data_processing,
            ArmFormat::SWI => Core::exec_arm_swi,
            ArmFormat::LDR_STR => Core::exec_arm_ldr_str,
            ArmFormat::LDR_STR_HS_IMM => Core::exec_arm_ldr_str_hs,
            ArmFormat::LDR_STR_HS_REG => Core::exec_arm_ldr_str_hs,
            ArmFormat::LDM_STM => Core::exec_arm_ldm_stm,
            ArmFormat::MRS => Core::exec_arm_mrs,
            ArmFormat::MSR_REG => Core::exec_arm_msr_reg,
            ArmFormat::MSR_FLAGS => Core::exec_arm_msr_flags,
            ArmFormat::MUL_MLA => Core::exec_arm_mul_mla,
            ArmFormat::MULL_MLAL => Core::exec_arm_mull_mlal,
            ArmFormat::SWP => Core::exec_arm_swp,
            ArmFormat::Undefined => |_, _, insn| {
                panic!(
                    "executing undefind thumb instruction {:08x} at @{:08x}",
                    insn.raw, insn.pc
                )
            },
        }
    }
}

pub struct ArmInstructionInfo {
    pub fmt: ArmFormat,
    pub handler_fn: ArmInstructionHandler,
}

impl ArmInstructionInfo {
    fn new(fmt: ArmFormat, handler_fn: ArmInstructionHandler) -> ArmInstructionInfo {
        ArmInstructionInfo { fmt, handler_fn }
    }
}

#[inline(always)]
pub fn arm_insn_hash(insn: u32) -> usize {
    (((insn >> 16) & 0xff0) | ((insn >> 4) & 0x00f)) as usize
}

impl From<u32> for ArmFormat {
    fn from(i: u32) -> ArmFormat {
        use ArmFormat::*;
        if (0x0ff0_00f0 & i) == 0x0120_0010 {
            BX
        } else if (0x0e00_0000 & i) == 0x0a00_0000 {
            B_BL
        } else if (0x0fb0_00f0 & i) == 0x0100_0090 {
            SWP
        } else if (0x0fc0_00f0 & i) == 0x0000_0090 {
            MUL_MLA
        } else if (0x0f80_00f0 & i) == 0x0000_0090 {
            MULL_MLAL
        } else if (0x0fb0_00f0 & i) == 0x0100_0000 {
            MRS
        } else if (0x0fb0_00f0 & i) == 0x0120_0000 {
            MSR_REG
        } else if (0x0db0_0000 & i) == 0x0120_0000 {
            MSR_FLAGS
        } else if (0x0c00_0000 & i) == 0x0400_0000 {
            LDR_STR
        } else if (0x0e40_0090 & i) == 0x0000_0090 {
            LDR_STR_HS_REG
        } else if (0x0e40_0090 & i) == 0x0000_0090 {
            LDR_STR_HS_IMM
        } else if (0x0e00_0000 & i) == 0x0800_0000 {
            LDM_STM
        } else if (0x0f00_0000 & i) == 0x0f00_0000 {
            SWI
        } else if (0x0c00_0000 & i) == 0x0000_0000 {
            DP
        } else {
            Undefined
        }
    }
}

lazy_static! {
    // there are 0xfff different hashes
    pub static ref ARM_LUT: [ArmInstructionInfo; 4096] = {
        use std::mem::{self, MaybeUninit};

        let mut lut: [MaybeUninit<ArmInstructionInfo>; 4096] = unsafe {
            MaybeUninit::uninit().assume_init()
        };

        for i in 0..4096 {
            let x = ((i & 0xff0) << 16) | ((i & 0x00f) << 4);
            let fmt = ArmFormat::from(x);
            // println!("{:#x} = {:?}", i, fmt);
            let info = ArmInstructionInfo::new(fmt, fmt.into());
            lut[i as usize] = MaybeUninit::new(info);
        }

        // HACK since my decoding order sucks
        // lut[0x121] = MaybeUninit::new(ArmInstructionInfo::new(ArmFormat::BX, Core::exec_arm_bx));

        // Everything is initialized. Transmute the array to the
        // initialized type.
        unsafe { mem::transmute::<_, [ArmInstructionInfo; 4096]>(lut) }
    };
}
