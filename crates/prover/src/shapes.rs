use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    fs::File,
    hash::{DefaultHasher, Hash, Hasher},
    panic::{catch_unwind, AssertUnwindSafe},
    path::PathBuf,
    sync::{Arc, Mutex},
};

use eyre::Result;
use thiserror::Error;

use p3_field::FieldAlgebra;
use p3_koala_bear::KoalaBear;
use serde::{Deserialize, Serialize};
use zkm_core_machine::shape::CoreShapeConfig;
use zkm_recursion_circuit::machine::{
    ZKMCompressWithVKeyWitnessValues, ZKMCompressWithVkeyShape, ZKMDeferredShape,
    ZKMDeferredWitnessValues, ZKMRecursionShape, ZKMRecursionWitnessValues,
};
use zkm_recursion_core::{
    shape::{RecursionShape, RecursionShapeConfig},
    RecursionProgram,
};
use zkm_stark::{shape::OrderedShape, MachineProver, DIGEST_SIZE};

use crate::{components::ZKMProverComponents, CompressAir, HashableKey, ShrinkAir, ZKMProver};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ZKMProofShape {
    Recursion(OrderedShape),
    Compress(Vec<OrderedShape>),
    Deferred(OrderedShape),
    Shrink(OrderedShape),
}

#[derive(Debug, Clone, Hash)]
pub enum ZKMCompressProgramShape {
    Recursion(ZKMRecursionShape),
    Compress(ZKMCompressWithVkeyShape),
    Deferred(ZKMDeferredShape),
    Shrink(ZKMCompressWithVkeyShape),
}

impl ZKMCompressProgramShape {
    pub fn hash_u64(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        Hash::hash(&self, &mut hasher);
        hasher.finish()
    }
}

#[derive(Debug, Error)]
pub enum VkBuildError {
    #[error("IO error: {0}")]
    IO(#[from] std::io::Error),
    #[error("Serialization error: {0}")]
    Bincode(#[from] bincode::Error),
}

pub fn check_shapes<C: ZKMProverComponents>(
    reduce_batch_size: usize,
    no_precompiles: bool,
    num_compiler_workers: usize,
    prover: &ZKMProver<C>,
) -> bool {
    let (shape_tx, shape_rx) =
        std::sync::mpsc::sync_channel::<ZKMCompressProgramShape>(num_compiler_workers);
    let (panic_tx, panic_rx) = std::sync::mpsc::channel();
    let core_shape_config = prover.core_shape_config.as_ref().expect("core shape config not found");
    let recursion_shape_config =
        prover.compress_shape_config.as_ref().expect("recursion shape config not found");

    let shape_rx = Mutex::new(shape_rx);

    let all_maximal_shapes = ZKMProofShape::generate_maximal_shapes(
        core_shape_config,
        recursion_shape_config,
        reduce_batch_size,
        no_precompiles,
    )
    .collect::<BTreeSet<ZKMProofShape>>();
    let num_shapes = all_maximal_shapes.len();
    tracing::info!("number of shapes: {}", num_shapes);

    // The Merkle tree height.
    let height = num_shapes.next_power_of_two().ilog2() as usize;

    let compress_ok = std::thread::scope(|s| {
        // Initialize compiler workers.
        for _ in 0..num_compiler_workers {
            let shape_rx = &shape_rx;
            let prover = &prover;
            let panic_tx = panic_tx.clone();
            s.spawn(move || {
                while let Ok(shape) = shape_rx.lock().unwrap().recv() {
                    tracing::info!("shape is {:?}", shape);
                    let program = catch_unwind(AssertUnwindSafe(|| {
                        // Try to build the recursion program from the given shape.
                        prover.program_from_shape(shape.clone(), None)
                    }));
                    match program {
                        Ok(_) => {}
                        Err(e) => {
                            tracing::warn!(
                                "Program generation failed for shape {:?}, with error: {:?}",
                                shape,
                                e
                            );
                            panic_tx.send(true).unwrap();
                        }
                    }
                }
            });
        }

        // Generate shapes and send them to the compiler workers.
        all_maximal_shapes.into_iter().for_each(|program_shape| {
            shape_tx
                .send(ZKMCompressProgramShape::from_proof_shape(program_shape, height))
                .unwrap();
        });

        drop(shape_tx);
        drop(panic_tx);

        // If the panic receiver has no panics, then the shape is correct.
        panic_rx.iter().next().is_none()
    });

    compress_ok
}

pub fn build_vk_map<C: ZKMProverComponents>(
    reduce_batch_size: usize,
    dummy: bool,
    num_compiler_workers: usize,
    num_setup_workers: usize,
    indices: Option<Vec<usize>>,
) -> (BTreeSet<[KoalaBear; DIGEST_SIZE]>, Vec<usize>, usize) {
    let mut prover = ZKMProver::<C>::new();
    prover.vk_verification = !dummy;
    let core_shape_config = prover.core_shape_config.as_ref().expect("core shape config not found");
    let recursion_shape_config =
        prover.compress_shape_config.as_ref().expect("recursion shape config not found");

    tracing::info!("building compress vk map");
    let (vk_set, panic_indices, height) = if dummy {
        tracing::warn!("Making a dummy vk map");
        let dummy_set = ZKMProofShape::dummy_vk_map(
            core_shape_config,
            recursion_shape_config,
            reduce_batch_size,
        )
        .into_keys()
        .collect::<BTreeSet<_>>();
        let height = dummy_set.len().next_power_of_two().ilog2() as usize;
        (dummy_set, vec![], height)
    } else {
        let (vk_tx, vk_rx) = std::sync::mpsc::channel();
        let (shape_tx, shape_rx) =
            std::sync::mpsc::sync_channel::<(usize, ZKMCompressProgramShape)>(num_compiler_workers);
        let (program_tx, program_rx) = std::sync::mpsc::sync_channel(num_setup_workers);
        let (panic_tx, panic_rx) = std::sync::mpsc::channel();

        let shape_rx = Mutex::new(shape_rx);
        let program_rx = Mutex::new(program_rx);

        let indices_set = indices.map(|indices| indices.into_iter().collect::<HashSet<_>>());
        let all_shapes =
            ZKMProofShape::generate(core_shape_config, recursion_shape_config, reduce_batch_size)
                .collect::<BTreeSet<_>>();
        let num_shapes = all_shapes.len();
        tracing::info!("number of shapes: {}", num_shapes);

        let height = num_shapes.next_power_of_two().ilog2() as usize;
        let chunk_size = indices_set.as_ref().map(|indices| indices.len()).unwrap_or(num_shapes);

        std::thread::scope(|s| {
            // Initialize compiler workers.
            for _ in 0..num_compiler_workers {
                let program_tx = program_tx.clone();
                let shape_rx = &shape_rx;
                let prover = &prover;
                let panic_tx = panic_tx.clone();
                s.spawn(move || {
                    while let Ok((i, shape)) = shape_rx.lock().unwrap().recv() {
                        println!("shape {i} is {shape:?}");
                        let program = catch_unwind(AssertUnwindSafe(|| {
                            prover.program_from_shape(shape.clone(), None)
                        }));
                        let is_shrink = matches!(shape, ZKMCompressProgramShape::Shrink(_));
                        match program {
                            Ok(program) => program_tx.send((i, program, is_shrink)).unwrap(),
                            Err(e) => {
                                tracing::warn!(
                                    "Program generation failed for shape {} {:?}, with error: {:?}",
                                    i,
                                    shape,
                                    e
                                );
                                panic_tx.send(i).unwrap();
                            }
                        }
                    }
                });
            }

            // Initialize setup workers.
            for _ in 0..num_setup_workers {
                let vk_tx = vk_tx.clone();
                let program_rx = &program_rx;
                let prover = &prover;
                s.spawn(move || {
                    let mut done = 0;
                    while let Ok((i, program, is_shrink)) = program_rx.lock().unwrap().recv() {
                        let vk = tracing::debug_span!("setup for program {}", i).in_scope(|| {
                            if is_shrink {
                                prover.shrink_prover.setup(&program).1
                            } else {
                                prover.compress_prover.setup(&program).1
                            }
                        });
                        done += 1;

                        let vk_digest = vk.hash_koalabear();
                        tracing::info!(
                            "program {} = {:?}, {}% done",
                            i,
                            vk_digest,
                            done * 100 / chunk_size
                        );
                        vk_tx.send(vk_digest).unwrap();
                    }
                });
            }

            // Generate shapes and send them to the compiler workers.
            let subset_shapes = all_shapes
                .into_iter()
                .enumerate()
                .filter(|(i, _)| indices_set.as_ref().map(|set| set.contains(i)).unwrap_or(true))
                .collect::<Vec<_>>();

            subset_shapes
                .clone()
                .into_iter()
                .map(|(i, shape)| (i, ZKMCompressProgramShape::from_proof_shape(shape, height)))
                .for_each(|(i, program_shape)| {
                    shape_tx.send((i, program_shape)).unwrap();
                });

            drop(shape_tx);
            drop(program_tx);
            drop(vk_tx);
            drop(panic_tx);

            let vk_set = vk_rx.iter().collect::<BTreeSet<_>>();

            let panic_indices = panic_rx.iter().collect::<Vec<_>>();

            for (i, shape) in subset_shapes {
                if panic_indices.contains(&i) {
                    tracing::info!("panic shape {}: {:?}", i, shape);
                }
            }

            (vk_set, panic_indices, height)
        })
    };
    tracing::info!("compress vks generated, number of keys: {}", vk_set.len());
    (vk_set, panic_indices, height)
}

pub fn build_vk_map_to_file<C: ZKMProverComponents>(
    build_dir: PathBuf,
    reduce_batch_size: usize,
    dummy: bool,
    num_compiler_workers: usize,
    num_setup_workers: usize,
    range_start: Option<usize>,
    range_end: Option<usize>,
) -> Result<(), VkBuildError> {
    std::fs::create_dir_all(&build_dir)?;

    tracing::info!("Building vk set");

    let (vk_set, _, _) = build_vk_map::<C>(
        reduce_batch_size,
        dummy,
        num_compiler_workers,
        num_setup_workers,
        range_start.and_then(|start| range_end.map(|end| (start..end).collect())),
    );

    let vk_map = vk_set.into_iter().enumerate().map(|(i, vk)| (vk, i)).collect::<BTreeMap<_, _>>();

    tracing::info!("Save the vk set to file");
    let mut file = if dummy {
        File::create(build_dir.join("dummy_vk_map.bin"))?
    } else {
        File::create(build_dir.join("vk_map.bin"))?
    };
    Ok(bincode::serialize_into(&mut file, &vk_map)?)
}

impl ZKMProofShape {
    pub fn generate<'a>(
        core_shape_config: &'a CoreShapeConfig<KoalaBear>,
        recursion_shape_config: &'a RecursionShapeConfig<KoalaBear, CompressAir<KoalaBear>>,
        reduce_batch_size: usize,
    ) -> impl Iterator<Item = Self> + 'a {
        core_shape_config
            .all_shapes()
            .map(Self::Recursion)
            .chain((1..=reduce_batch_size).flat_map(|batch_size| {
                recursion_shape_config.get_all_shape_combinations(batch_size).map(Self::Compress)
            }))
            .chain(
                recursion_shape_config
                    .get_all_shape_combinations(1)
                    .map(|mut x| Self::Deferred(x.pop().unwrap())),
            )
            .chain(
                recursion_shape_config
                    .get_all_shape_combinations(1)
                    .map(|mut x| Self::Shrink(x.pop().unwrap())),
            )
    }

    pub fn generate_compress_shapes(
        recursion_shape_config: &'_ RecursionShapeConfig<KoalaBear, CompressAir<KoalaBear>>,
        reduce_batch_size: usize,
    ) -> impl Iterator<Item = Vec<OrderedShape>> + '_ {
        recursion_shape_config.get_all_shape_combinations(reduce_batch_size)
    }

    pub fn generate_maximal_shapes<'a>(
        core_shape_config: &'a CoreShapeConfig<KoalaBear>,
        recursion_shape_config: &'a RecursionShapeConfig<KoalaBear, CompressAir<KoalaBear>>,
        reduce_batch_size: usize,
        no_precompiles: bool,
    ) -> impl Iterator<Item = Self> + 'a {
        let core_shape_iter = if no_precompiles {
            core_shape_config.maximal_core_shapes(21).into_iter()
        } else {
            core_shape_config.maximal_core_plus_precompile_shapes(21).into_iter()
        };
        core_shape_iter
            .map(|core_shape| {
                Self::Recursion(OrderedShape {
                    inner: core_shape.into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
                })
            })
            .chain((1..=reduce_batch_size).flat_map(|batch_size| {
                recursion_shape_config.get_all_shape_combinations(batch_size).map(Self::Compress)
            }))
            .chain(
                recursion_shape_config
                    .get_all_shape_combinations(1)
                    .map(|mut x| Self::Deferred(x.pop().unwrap())),
            )
            .chain(
                recursion_shape_config
                    .get_all_shape_combinations(1)
                    .map(|mut x| Self::Shrink(x.pop().unwrap())),
            )
    }

    pub fn dummy_vk_map<'a>(
        core_shape_config: &'a CoreShapeConfig<KoalaBear>,
        recursion_shape_config: &'a RecursionShapeConfig<KoalaBear, CompressAir<KoalaBear>>,
        reduce_batch_size: usize,
    ) -> BTreeMap<[KoalaBear; DIGEST_SIZE], usize> {
        Self::generate(core_shape_config, recursion_shape_config, reduce_batch_size)
            .enumerate()
            .map(|(i, _)| ([KoalaBear::from_canonical_usize(i); DIGEST_SIZE], i))
            .collect()
    }
}

impl ZKMCompressProgramShape {
    pub fn from_proof_shape(shape: ZKMProofShape, height: usize) -> Self {
        match shape {
            ZKMProofShape::Recursion(proof_shape) => Self::Recursion(proof_shape.into()),
            ZKMProofShape::Deferred(proof_shape) => {
                Self::Deferred(ZKMDeferredShape::new(vec![proof_shape].into(), height))
            }
            ZKMProofShape::Compress(proof_shapes) => Self::Compress(ZKMCompressWithVkeyShape {
                compress_shape: proof_shapes.into(),
                merkle_tree_height: height,
            }),
            ZKMProofShape::Shrink(proof_shape) => Self::Shrink(ZKMCompressWithVkeyShape {
                compress_shape: vec![proof_shape].into(),
                merkle_tree_height: height,
            }),
        }
    }
}

impl<C: ZKMProverComponents> ZKMProver<C> {
    pub fn program_from_shape(
        &self,
        shape: ZKMCompressProgramShape,
        shrink_shape: Option<RecursionShape>,
    ) -> Arc<RecursionProgram<KoalaBear>> {
        match shape {
            ZKMCompressProgramShape::Recursion(shape) => {
                let input = ZKMRecursionWitnessValues::dummy(self.core_prover.machine(), &shape);
                self.recursion_program(&input)
            }
            ZKMCompressProgramShape::Deferred(shape) => {
                let input = ZKMDeferredWitnessValues::dummy(self.compress_prover.machine(), &shape);
                self.deferred_program(&input)
            }
            ZKMCompressProgramShape::Compress(shape) => {
                let input =
                    ZKMCompressWithVKeyWitnessValues::dummy(self.compress_prover.machine(), &shape);
                self.compress_program(&input)
            }
            ZKMCompressProgramShape::Shrink(shape) => {
                let input =
                    ZKMCompressWithVKeyWitnessValues::dummy(self.compress_prover.machine(), &shape);
                self.shrink_program(
                    shrink_shape.unwrap_or_else(ShrinkAir::<KoalaBear>::shrink_shape),
                    &input,
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore]
    fn test_generate_all_shapes() {
        let core_shape_config = CoreShapeConfig::default();
        let recursion_shape_config = RecursionShapeConfig::default();
        let reduce_batch_size = 2;
        let all_shapes =
            ZKMProofShape::generate(&core_shape_config, &recursion_shape_config, reduce_batch_size)
                .collect::<BTreeSet<_>>();

        println!("Number of compress shapes: {}", all_shapes.len());
    }
}
