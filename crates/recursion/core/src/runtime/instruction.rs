use std::borrow::Borrow;

use p3_field::{FieldAlgebra, FieldExtensionAlgebra};
use serde::{Deserialize, Serialize};

use crate::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Instruction<F> {
    BaseAlu(BaseAluInstr<F>),
    ExtAlu(ExtAluInstr<F>),
    Mem(MemInstr<F>),
    Poseidon2(Box<Poseidon2Instr<F>>),
    Select(SelectInstr<F>),
    ExpReverseBitsLen(ExpReverseBitsInstr<F>),
    HintBits(HintBitsInstr<F>),
    HintAddCurve(HintAddCurveInstr<F>),
    FriFold(Box<FriFoldInstr<F>>),
    BatchFRI(Box<BatchFRIInstr<F>>),
    Print(PrintInstr<F>),
    HintExt2Felts(HintExt2FeltsInstr<F>),
    CommitPublicValues(Box<CommitPublicValuesInstr<F>>),
    Hint(HintInstr<F>),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HintBitsInstr<F> {
    /// Addresses and mults of the output bits.
    pub output_addrs_mults: Vec<(Address<F>, F)>,
    /// Input value to decompose.
    pub input_addr: Address<F>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PrintInstr<F> {
    pub field_elt_type: FieldEltType,
    pub addr: Address<F>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HintAddCurveInstr<F> {
    pub output_x_addrs_mults: Vec<(Address<F>, F)>,
    pub output_y_addrs_mults: Vec<(Address<F>, F)>,
    pub input1_x_addrs: Vec<Address<F>>,
    pub input1_y_addrs: Vec<Address<F>>,
    pub input2_x_addrs: Vec<Address<F>>,
    pub input2_y_addrs: Vec<Address<F>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HintInstr<F> {
    /// Addresses and mults of the output felts.
    pub output_addrs_mults: Vec<(Address<F>, F)>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HintExt2FeltsInstr<F> {
    /// Addresses and mults of the output bits.
    pub output_addrs_mults: [(Address<F>, F); D],
    /// Input value to decompose.
    pub input_addr: Address<F>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum FieldEltType {
    Base,
    Extension,
}

pub fn base_alu<F: FieldAlgebra>(
    opcode: BaseAluOpcode,
    mult: u32,
    out: u32,
    in1: u32,
    in2: u32,
) -> Instruction<F> {
    Instruction::BaseAlu(BaseAluInstr {
        opcode,
        mult: F::from_canonical_u32(mult),
        addrs: BaseAluIo {
            out: Address(F::from_canonical_u32(out)),
            in1: Address(F::from_canonical_u32(in1)),
            in2: Address(F::from_canonical_u32(in2)),
        },
    })
}

pub fn ext_alu<F: FieldAlgebra>(
    opcode: ExtAluOpcode,
    mult: u32,
    out: u32,
    in1: u32,
    in2: u32,
) -> Instruction<F> {
    Instruction::ExtAlu(ExtAluInstr {
        opcode,
        mult: F::from_canonical_u32(mult),
        addrs: ExtAluIo {
            out: Address(F::from_canonical_u32(out)),
            in1: Address(F::from_canonical_u32(in1)),
            in2: Address(F::from_canonical_u32(in2)),
        },
    })
}

pub fn mem<F: FieldAlgebra>(kind: MemAccessKind, mult: u32, addr: u32, val: u32) -> Instruction<F> {
    mem_single(kind, mult, addr, F::from_canonical_u32(val))
}

pub fn mem_single<F: FieldAlgebra>(
    kind: MemAccessKind,
    mult: u32,
    addr: u32,
    val: F,
) -> Instruction<F> {
    mem_block(kind, mult, addr, Block::from(val))
}

pub fn mem_ext<F: FieldAlgebra + Copy, EF: FieldExtensionAlgebra<F>>(
    kind: MemAccessKind,
    mult: u32,
    addr: u32,
    val: EF,
) -> Instruction<F> {
    mem_block(kind, mult, addr, val.as_base_slice().into())
}

pub fn mem_block<F: FieldAlgebra>(
    kind: MemAccessKind,
    mult: u32,
    addr: u32,
    val: Block<F>,
) -> Instruction<F> {
    Instruction::Mem(MemInstr {
        addrs: MemIo { inner: Address(F::from_canonical_u32(addr)) },
        vals: MemIo { inner: val },
        mult: F::from_canonical_u32(mult),
        kind,
    })
}

pub fn poseidon2<F: FieldAlgebra>(
    mults: [u32; WIDTH],
    output: [u32; WIDTH],
    input: [u32; WIDTH],
) -> Instruction<F> {
    Instruction::Poseidon2(Box::new(Poseidon2Instr {
        mults: mults.map(F::from_canonical_u32),
        addrs: Poseidon2Io {
            output: output.map(F::from_canonical_u32).map(Address),
            input: input.map(F::from_canonical_u32).map(Address),
        },
    }))
}

#[allow(clippy::too_many_arguments)]
pub fn select<F: FieldAlgebra>(
    mult1: u32,
    mult2: u32,
    bit: u32,
    out1: u32,
    out2: u32,
    in1: u32,
    in2: u32,
) -> Instruction<F> {
    Instruction::Select(SelectInstr {
        mult1: F::from_canonical_u32(mult1),
        mult2: F::from_canonical_u32(mult2),
        addrs: SelectIo {
            bit: Address(F::from_canonical_u32(bit)),
            out1: Address(F::from_canonical_u32(out1)),
            out2: Address(F::from_canonical_u32(out2)),
            in1: Address(F::from_canonical_u32(in1)),
            in2: Address(F::from_canonical_u32(in2)),
        },
    })
}

pub fn exp_reverse_bits_len<F: FieldAlgebra>(
    mult: u32,
    base: F,
    exp: Vec<F>,
    result: F,
) -> Instruction<F> {
    Instruction::ExpReverseBitsLen(ExpReverseBitsInstr {
        mult: F::from_canonical_u32(mult),
        addrs: ExpReverseBitsIo {
            base: Address(base),
            exp: exp.into_iter().map(Address).collect(),
            result: Address(result),
        },
    })
}

#[allow(clippy::too_many_arguments)]
pub fn fri_fold<F: FieldAlgebra>(
    z: u32,
    alpha: u32,
    x: u32,
    mat_opening: Vec<u32>,
    ps_at_z: Vec<u32>,
    alpha_pow_input: Vec<u32>,
    ro_input: Vec<u32>,
    alpha_pow_output: Vec<u32>,
    ro_output: Vec<u32>,
    alpha_mults: Vec<u32>,
    ro_mults: Vec<u32>,
) -> Instruction<F> {
    Instruction::FriFold(Box::new(FriFoldInstr {
        base_single_addrs: FriFoldBaseIo { x: Address(F::from_canonical_u32(x)) },
        ext_single_addrs: FriFoldExtSingleIo {
            z: Address(F::from_canonical_u32(z)),
            alpha: Address(F::from_canonical_u32(alpha)),
        },
        ext_vec_addrs: FriFoldExtVecIo {
            mat_opening: mat_opening
                .iter()
                .map(|elm| Address(F::from_canonical_u32(*elm)))
                .collect(),
            ps_at_z: ps_at_z.iter().map(|elm| Address(F::from_canonical_u32(*elm))).collect(),
            alpha_pow_input: alpha_pow_input
                .iter()
                .map(|elm| Address(F::from_canonical_u32(*elm)))
                .collect(),
            ro_input: ro_input.iter().map(|elm| Address(F::from_canonical_u32(*elm))).collect(),
            alpha_pow_output: alpha_pow_output
                .iter()
                .map(|elm| Address(F::from_canonical_u32(*elm)))
                .collect(),
            ro_output: ro_output.iter().map(|elm| Address(F::from_canonical_u32(*elm))).collect(),
        },
        alpha_pow_mults: alpha_mults.iter().map(|mult| F::from_canonical_u32(*mult)).collect(),
        ro_mults: ro_mults.iter().map(|mult| F::from_canonical_u32(*mult)).collect(),
    }))
}

#[allow(clippy::too_many_arguments)]
pub fn batch_fri<F: FieldAlgebra>(
    acc: u32,
    alpha_pows: Vec<u32>,
    p_at_zs: Vec<u32>,
    p_at_xs: Vec<u32>,
    acc_mult: u32,
) -> Instruction<F> {
    Instruction::BatchFRI(Box::new(BatchFRIInstr {
        base_vec_addrs: BatchFRIBaseVecIo {
            p_at_x: p_at_xs.iter().map(|elm| Address(F::from_canonical_u32(*elm))).collect(),
        },
        ext_single_addrs: BatchFRIExtSingleIo { acc: Address(F::from_canonical_u32(acc)) },
        ext_vec_addrs: BatchFRIExtVecIo {
            p_at_z: p_at_zs.iter().map(|elm| Address(F::from_canonical_u32(*elm))).collect(),
            alpha_pow: alpha_pows.iter().map(|elm| Address(F::from_canonical_u32(*elm))).collect(),
        },
        acc_mult: F::from_canonical_u32(acc_mult),
    }))
}

pub fn commit_public_values<F: FieldAlgebra>(
    public_values_a: &RecursionPublicValues<u32>,
) -> Instruction<F> {
    let pv_a = public_values_a.as_array().map(|pv| Address(F::from_canonical_u32(pv)));
    let pv_address: &RecursionPublicValues<Address<F>> = pv_a.as_slice().borrow();

    Instruction::CommitPublicValues(Box::new(CommitPublicValuesInstr {
        pv_addrs: pv_address.clone(),
    }))
}
