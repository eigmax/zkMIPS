#![no_std]
#![no_main]
zkm_zkvm::entrypoint!(main);

use zkm_zkvm::syscalls::syscall_secp256r1_double;

pub fn main() {
    // generator.
    // 48439561293906451759052585252797914202762949526041747995844080717082404635286
    // 36134250956749795798585127919587881956611106672985015071877198253568414405109
    let mut a: [u8; 64] = [
        150, 194, 152, 216, 69, 57, 161, 244, 160, 51, 235, 45, 129, 125, 3, 119, 242, 64, 164, 99,
        229, 230, 188, 248, 71, 66, 44, 225, 242, 209, 23, 107, 245, 81, 191, 55, 104, 64, 182,
        203, 206, 94, 49, 107, 87, 51, 206, 43, 22, 158, 15, 124, 74, 235, 231, 142, 155, 127, 26,
        254, 226, 66, 227, 79,
    ];

    // 2 * generator.
    // 56515219790691171413109057904011688695424810155802929973526481321309856242040
    // 3377031843712258259223711451491452598088675519751548567112458094635497583569
    let b: [u8; 64] = [
        120, 153, 102, 71, 252, 72, 11, 166, 53, 27, 242, 119, 226, 105, 137, 192, 195, 26, 181, 4,
        3, 56, 82, 138, 126, 79, 3, 141, 24, 123, 242, 124, 209, 115, 120, 34, 157, 183, 4, 158,
        41, 130, 233, 60, 230, 173, 125, 186, 219, 48, 116, 159, 198, 154, 61, 41, 64, 208, 142,
        219, 16, 85, 119, 7,
    ];

    syscall_secp256r1_double(a.as_mut_ptr() as *mut [u32; 16]);

    assert_eq!(a, b);
}
