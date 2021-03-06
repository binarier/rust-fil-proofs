use clap::{values_t, App, Arg};
use log::info;
use paired::bls12_381::Bls12;

use filecoin_proofs::constants::*;
use filecoin_proofs::parameters::{post_public_params, public_params};
use filecoin_proofs::types::*;
use std::collections::HashSet;
use storage_proofs::circuit::election_post::{ElectionPoStCircuit, ElectionPoStCompound};
use storage_proofs::circuit::stacked::StackedCompound;
use storage_proofs::compound_proof::CompoundProof;
use storage_proofs::election_post::ElectionPoSt;
use storage_proofs::hasher::pedersen::PedersenHasher;
use storage_proofs::parameter_cache::CacheableParameters;
use storage_proofs::stacked::StackedDrg;

const PUBLISHED_SECTOR_SIZES: [u64; 5] = [
    SECTOR_SIZE_ONE_KIB,
    SECTOR_SIZE_16_MIB,
    SECTOR_SIZE_256_MIB,
    SECTOR_SIZE_1_GIB,
    SECTOR_SIZE_32_GIB,
];

fn cache_porep_params(porep_config: PoRepConfig) {
    let n = u64::from(PaddedBytesAmount::from(porep_config));
    info!(
        "begin PoRep parameter-cache check/populate routine for {}-byte sectors",
        n
    );

    let public_params = public_params(
        PaddedBytesAmount::from(porep_config),
        usize::from(PoRepProofPartitions::from(porep_config)),
    )
    .unwrap();

    {
        let circuit = <StackedCompound<DefaultTreeHasher, DefaultPieceHasher> as CompoundProof<
            _,
            StackedDrg<DefaultTreeHasher, DefaultPieceHasher>,
            _,
        >>::blank_circuit(&public_params);
        let _ = StackedCompound::<DefaultTreeHasher, DefaultPieceHasher>::get_param_metadata(
            circuit,
            &public_params,
        );
    }
    {
        let circuit = <StackedCompound<DefaultTreeHasher, DefaultPieceHasher> as CompoundProof<
            _,
            StackedDrg<DefaultTreeHasher, DefaultPieceHasher>,
            _,
        >>::blank_circuit(&public_params);
        StackedCompound::<DefaultTreeHasher, DefaultPieceHasher>::get_groth_params(
            circuit,
            &public_params,
        )
        .expect("failed to get groth params");
    }
    {
        let circuit = <StackedCompound<DefaultTreeHasher, DefaultPieceHasher> as CompoundProof<
            _,
            StackedDrg<DefaultTreeHasher, DefaultPieceHasher>,
            _,
        >>::blank_circuit(&public_params);

        StackedCompound::<DefaultTreeHasher, DefaultPieceHasher>::get_verifying_key(
            circuit,
            &public_params,
        )
        .expect("failed to get verifying key");
    }
}

fn cache_post_params(post_config: PoStConfig) {
    let n = u64::from(PaddedBytesAmount::from(post_config));
    info!(
        "begin PoSt parameter-cache check/populate routine for {}-byte sectors",
        n
    );

    let post_public_params = post_public_params(post_config).unwrap();

    {
        let post_circuit: ElectionPoStCircuit<Bls12, PedersenHasher> =
            <ElectionPoStCompound<PedersenHasher> as CompoundProof<
                Bls12,
                ElectionPoSt<PedersenHasher>,
                ElectionPoStCircuit<Bls12, PedersenHasher>,
            >>::blank_circuit(&post_public_params);
        let _ = <ElectionPoStCompound<PedersenHasher>>::get_param_metadata(
            post_circuit,
            &post_public_params,
        )
        .expect("failed to get metadata");
    }
    {
        let post_circuit: ElectionPoStCircuit<Bls12, PedersenHasher> =
            <ElectionPoStCompound<PedersenHasher> as CompoundProof<
                Bls12,
                ElectionPoSt<PedersenHasher>,
                ElectionPoStCircuit<Bls12, PedersenHasher>,
            >>::blank_circuit(&post_public_params);
        <ElectionPoStCompound<PedersenHasher>>::get_groth_params(post_circuit, &post_public_params)
            .expect("failed to get groth params");
    }
    {
        let post_circuit: ElectionPoStCircuit<Bls12, PedersenHasher> =
            <ElectionPoStCompound<PedersenHasher> as CompoundProof<
                Bls12,
                ElectionPoSt<PedersenHasher>,
                ElectionPoStCircuit<Bls12, PedersenHasher>,
            >>::blank_circuit(&post_public_params);

        <ElectionPoStCompound<PedersenHasher>>::get_verifying_key(
            post_circuit,
            &post_public_params,
        )
        .expect("failed to get verifying key");
    }
}

// Run this from the command-line to pre-generate the groth parameters used by the API.
pub fn main() {
    fil_logger::init();

    let matches = App::new("paramcache")
        .version("0.1")
        .about("Generate and persist Groth parameters and verifying keys")
        .arg(
            Arg::with_name("params-for-sector-sizes")
                .short("z")
                .long("params-for-sector-sizes")
                .conflicts_with("all")
                .require_delimiter(true)
                .value_delimiter(",")
                .multiple(true)
                .help("A comma-separated list of sector sizes, in bytes, for which Groth parameters will be generated")
        )
        .arg(
            Arg::with_name("only-election-post")
                .long("only-election-post")
                .help("Only generate parameters for election-post")
        )
        .get_matches();

    let sizes: HashSet<u64> = if matches.is_present("params-for-sector-sizes") {
        values_t!(matches.values_of("params-for-sector-sizes"), u64)
            .unwrap()
            .into_iter()
            .collect()
    } else {
        PUBLISHED_SECTOR_SIZES.iter().cloned().collect()
    };

    let only_election_post = matches.is_present("only-election-post");

    for sector_size in sizes {
        cache_post_params(PoStConfig {
            sector_size: SectorSize(sector_size),
            challenge_count: POST_CHALLENGE_COUNT,
            challenged_nodes: POST_CHALLENGED_NODES,
        });

        if !only_election_post {
            cache_porep_params(PoRepConfig {
                sector_size: SectorSize(sector_size),
                partitions: PoRepProofPartitions(
                    *POREP_PARTITIONS
                        .read()
                        .unwrap()
                        .get(&sector_size)
                        .expect("missing sector size"),
                ),
            });
        }
    }
}
