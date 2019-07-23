use bit::BitIndex;
use num::FromPrimitive;

use super::{Core, CpuError, CpuResult, REG_PC};

#[derive(Debug, Primitive, PartialEq)]
pub enum AluOpCode {
    AND = 0b0000,
    EOR = 0b0001,
    SUB = 0b0010,
    RSB = 0b0011,
    ADD = 0b0100,
    ADC = 0b0101,
    SBC = 0b0110,
    RSC = 0b0111,
    TST = 0b1000,
    TEQ = 0b1001,
    CMP = 0b1010,
    CMN = 0b1011,
    ORR = 0b1100,
    MOV = 0b1101,
    BIC = 0b1110,
    MVN = 0b1111,
}

impl AluOpCode {
    pub fn is_setting_flags(&self) -> bool {
        use AluOpCode::*;
        match self {
            TST | TEQ | CMP | CMN => true,
            _ => false,
        }
    }

    pub fn is_logical(&self) -> bool {
        use AluOpCode::*;
        match self {
            MOV | MVN | ORR | EOR | AND | BIC | TST | TEQ => true,
            _ => false,
        }
    }
    pub fn is_arithmetic(&self) -> bool {
        use AluOpCode::*;
        match self {
            ADD | ADC | SUB | SBC | RSB | RSC | CMP | CMN => true,
            _ => false,
        }
    }
}

#[derive(Debug, PartialEq, Primitive)]
pub enum BarrelShiftOpCode {
    LSL = 0,
    LSR = 1,
    ASR = 2,
    ROR = 3,
}

#[derive(Debug, PartialEq)]
pub enum ShiftRegisterBy {
    ByAmount(u32),
    ByRegister(usize),
}

#[derive(Debug, PartialEq)]
pub struct ShiftedRegister {
    pub reg: usize,
    pub shift_by: ShiftRegisterBy,
    pub bs_op: BarrelShiftOpCode,
    pub added: Option<bool>,
}

#[derive(Debug, PartialEq)]
pub enum BarrelShifterValue {
    ImmediateValue(i32),
    RotatedImmediate(u32, u32),
    ShiftedRegister(ShiftedRegister),
}

impl BarrelShifterValue {
    /// Decode operand2 as an immediate value
    pub fn decode_rotated_immediate(&self) -> Option<i32> {
        if let BarrelShifterValue::RotatedImmediate(immediate, rotate) = self {
            return Some(immediate.rotate_right(*rotate) as i32);
        }
        None
    }
}

#[derive(Debug, Default)]
pub struct BarrelShifter {
    pub carry_out: bool,
}

impl BarrelShifter {
    /// Performs a generic barrel shifter operation
    pub fn barrel_shift_op(
        &mut self,
        shift: BarrelShiftOpCode,
        val: i32,
        amount: u32,
        carry_in: bool,
        immediate: bool,
    ) -> i32 {
        //
        // From GBATEK:
        // Zero Shift Amount (Shift Register by Immediate, with Immediate=0)
        //  LSL#0: No shift performed, ie. directly Op2=Rm, the C flag is NOT affected.
        //  LSR#0: Interpreted as LSR#32, ie. Op2 becomes zero, C becomes Bit 31 of Rm.
        //  ASR#0: Interpreted as ASR#32, ie. Op2 and C are filled by Bit 31 of Rm.
        //  ROR#0: Interpreted as RRX#1 (RCR), like ROR#1, but Op2 Bit 31 set to old C.
        //
        // From ARM7TDMI Datasheet:
        // 1 LSL by 32 has result zero, carry out equal to bit 0 of Rm.
        // 2 LSL by more than 32 has result zero, carry out zero.
        // 3 LSR by 32 has result zero, carry out equal to bit 31 of Rm.
        // 4 LSR by more than 32 has result zero, carry out zero.
        // 5 ASR by 32 or more has result filled with and carry out equal to bit 31 of Rm.
        // 6 ROR by 32 has result equal to Rm, carry out equal to bit 31 of Rm.
        // 7 ROR by n where n is greater than 32 will give the same result and carry out
        //   as ROR by n-32; therefore repeatedly subtract 32 from n until the amount is
        //   in the range 1 to 32 and see above.
        //
        match shift {
            BarrelShiftOpCode::LSL => match amount {
                0 => {
                    self.carry_out = carry_in;
                    val
                },
                x if x < 32 => {
                    self.carry_out = val.wrapping_shr(32 - x) & 1 == 1;
                    val << x
                }
                32 => {
                    self.carry_out = val & 1 == 1;
                    0
                }
                _ => {
                    self.carry_out = false;
                    0
                }
            },
            BarrelShiftOpCode::LSR => match amount {
                0 | 32 => {
                    if immediate {
                        self.carry_out = (val as u32).bit(31);
                        0
                    } else {
                        val
                    }
                }
                x if x < 32 => {
                    self.carry_out = val >> (amount - 1) & 1 == 1;
                    ((val as u32) >> amount) as i32
                }
                _ => {
                    self.carry_out = false;
                    0
                }
            },
            BarrelShiftOpCode::ASR => match amount {
                0 => {
                    if immediate {
                        let bit31 = (val as u32).bit(31);
                        self.carry_out = bit31;
                        if bit31 {
                            -1
                        } else {
                            0
                        }
                    } else {
                        val
                    }
                }
                x if x < 32 => {
                    self.carry_out = val.wrapping_shr(amount - 1) & 1 == 1;
                    val.wrapping_shr(amount)
                }
                _ => {
                    let bit31 = (val as u32).bit(31);
                    self.carry_out = bit31;
                    if bit31 {
                        -1
                    } else {
                        0
                    }
                }
            },
            BarrelShiftOpCode::ROR => {
                match amount {
                    0 => {
                        if immediate {
                            /* RRX */
                            let old_c = carry_in as i32;
                            self.carry_out = val & 0b1 != 0;
                            ((val as u32) >> 1) as i32 | (old_c << 31)
                        } else {
                            val
                        }
                    }
                    _ => {
                        let amount = amount % 32;
                        let val = if amount != 0 {
                            val.rotate_right(amount)
                        } else {
                            val
                        };
                        self.carry_out = (val as u32).bit(31);
                        val
                    }
                }
            }
        }
    }
}

impl Core {
    pub fn register_shift(&mut self, shift: ShiftedRegister) -> CpuResult<i32> {
        let mut val = self.get_reg(shift.reg) as i32;
        let carry = self.cpsr.C();
        match shift.shift_by {
            ShiftRegisterBy::ByAmount(amount) => {
                let result = self
                    .bs
                    .barrel_shift_op(shift.bs_op, val, amount, carry, true);
                Ok(result)
            }
            ShiftRegisterBy::ByRegister(rs) => {
                if shift.reg == REG_PC {
                    val = val + 4; // PC prefetching
                }
                if rs != REG_PC {
                    let amount = self.get_reg(rs) & 0xff;
                    let result = self
                        .bs
                        .barrel_shift_op(shift.bs_op, val, amount, carry, false);
                    Ok(result)
                } else {
                    Err(CpuError::IllegalInstruction)
                }
            }
        }
    }

    pub fn get_barrel_shifted_value(&mut self, sval: BarrelShifterValue) -> i32 {
        // TODO decide if error handling or panic here
        match sval {
            BarrelShifterValue::ImmediateValue(offset) => offset,
            BarrelShifterValue::ShiftedRegister(shifted_reg) => {
                let added = shifted_reg.added.unwrap_or(true);
                let abs = self.register_shift(shifted_reg).unwrap();
                if added {
                    abs
                } else {
                    -abs
                }
            }
            _ => panic!("bad barrel shift"),
        }
    }

    fn alu_sub_flags(a: i32, b: i32, carry: &mut bool, overflow: &mut bool) -> i32 {
        let res = a.wrapping_sub(b);
        *carry = b <= a;
        let (_, would_overflow) = a.overflowing_sub(b);
        *overflow = would_overflow;
        res
    }

    fn alu_add_flags(a: i32, b: i32, carry: &mut bool, overflow: &mut bool) -> i32 {
        let res = a.wrapping_add(b) as u32;
        *carry = res < a as u32 || res < b as u32;
        let (_, would_overflow) = a.overflowing_add(b);
        *overflow = would_overflow;
        res as i32
    }

    #[allow(non_snake_case)]
    pub fn alu(&mut self, opcode: AluOpCode, op1: i32, op2: i32) -> i32 {
        use AluOpCode::*;
        let C = self.cpsr.C() as i32;

        match opcode {
            AND => op1 & op2,
            EOR => op1 ^ op2,
            SUB => op1.wrapping_sub(op2),
            RSB => op2.wrapping_sub(op1),
            ADD => op1.wrapping_add(op2),
            ADC => op1.wrapping_add(op2).wrapping_add(C),
            SBC => op1.wrapping_sub(op2).wrapping_sub(1 - C),
            RSC => op2.wrapping_sub(op1).wrapping_sub(1 - C),
            ORR => op1 | op2,
            MOV => op2,
            BIC => op1 & (!op2),
            MVN => !op2,
            _ => panic!("{} should be a PSR transfer", opcode),
        }
    }

    #[allow(non_snake_case)]
    pub fn alu_flags(&mut self, opcode: AluOpCode, op1: i32, op2: i32) -> Option<i32> {
        use AluOpCode::*;
        let mut carry = self.bs.carry_out;
        let C = self.cpsr.C() as i32;
        let mut overflow = self.cpsr.V();

        let result = match opcode {
            AND | TST => op1 & op2,
            EOR | TEQ => op1 ^ op2,
            SUB | CMP => Self::alu_sub_flags(op1, op2, &mut carry, &mut overflow),
            RSB => Self::alu_sub_flags(op2, op1, &mut carry, &mut overflow),
            ADD | CMN => Self::alu_add_flags(op1, op2, &mut carry, &mut overflow),
            ADC => Self::alu_add_flags(op1, op2.wrapping_add(C), &mut carry, &mut overflow),
            SBC => Self::alu_sub_flags(op1, op2, &mut carry, &mut overflow).wrapping_sub(1 - C),
            RSC => Self::alu_sub_flags(op2, op1, &mut carry, &mut overflow).wrapping_sub(1 - C),
            ORR => op1 | op2,
            MOV => op2,
            BIC => op1 & (!op2),
            MVN => !op2,
        };

        self.cpsr.set_N(result < 0);
        self.cpsr.set_Z(result == 0);
        self.cpsr.set_C(carry);
        if opcode.is_arithmetic() {
            self.cpsr.set_V(overflow);
        }

        if opcode.is_setting_flags() {
            None
        } else {
            Some(result)
        }
    }
}
