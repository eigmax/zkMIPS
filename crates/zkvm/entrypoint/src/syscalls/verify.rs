#[cfg(target_os = "zkvm")]
use core::arch::asm;

cfg_if::cfg_if! {
    if #[cfg(target_os = "zkvm")] {
        use crate::syscalls::VERIFY_ZKM_PROOF;
        use crate::zkvm::DEFERRED_PROOFS_DIGEST;
        use p3_koala_bear::KoalaBear;
        use p3_field::FieldAlgebra;
        use zkm_primitives::hash_deferred_proof;
    }
}

#[no_mangle]
#[allow(unused_variables)]
pub extern "C" fn syscall_verify_zkm_proof(vk_digest: &[u32; 8], pv_digest: &[u8; 32]) {
    #[cfg(target_os = "zkvm")]
    {
        // Call syscall to verify the next proof at runtime
        unsafe {
            asm!(
                "syscall",
                in("$2") VERIFY_ZKM_PROOF,
                in("$4") vk_digest.as_ptr(),
                in("$5") pv_digest.as_ptr(),
            );
        }

        // Update digest to p2_hash(prev_digest[0..8] || vkey_digest[0..8] || pv_digest[0..32])
        let mut hash_input = Vec::with_capacity(48);
        // First 8 elements are previous hash (initially zero)
        let deferred_proofs_digest;
        // SAFETY: we have sole access because zkvm is single threaded.
        unsafe {
            deferred_proofs_digest = DEFERRED_PROOFS_DIGEST.as_mut().unwrap();
        }
        hash_input.extend_from_slice(deferred_proofs_digest);

        // Next 8 elements are vkey_digest
        let vk_digest_koalabear =
            vk_digest.iter().map(|x| KoalaBear::from_canonical_u32(*x)).collect::<Vec<_>>();
        // Remaining 32 elements are pv_digest converted from u8 to KoalaBear
        let pv_digest_koalabear =
            pv_digest.iter().map(|b| KoalaBear::from_canonical_u8(*b)).collect::<Vec<_>>();

        *deferred_proofs_digest = hash_deferred_proof(
            deferred_proofs_digest,
            &vk_digest_koalabear.try_into().unwrap(),
            &pv_digest_koalabear.try_into().unwrap(),
        );
    }

    #[cfg(not(target_os = "zkvm"))]
    unreachable!()
}
