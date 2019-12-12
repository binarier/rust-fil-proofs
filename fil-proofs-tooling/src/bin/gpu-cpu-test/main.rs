use std::collections::BTreeMap;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use bellperson::gpu;
use clap::{value_t, App, Arg};
use filecoin_proofs::{Candidate, PrivateReplicaInfo};
use log::{debug, info, trace};
use storage_proofs::sector::SectorId;

mod election_post;

const TIMEOUT: u64 = 5 * 60;

#[derive(Debug)]
pub struct RunInfo {
    elapsed: Duration,
    iterations: u8,
}

pub fn colored_with_thread(
    writer: &mut dyn std::io::Write,
    now: &mut flexi_logger::DeferredNow,
    record: &flexi_logger::Record,
) -> Result<(), std::io::Error> {
    let level = record.level();
    write!(
        writer,
        "{} {} {} {} > {}",
        now.now().format("%Y-%m-%dT%H:%M:%S%.3f"),
        thread::current()
            .name()
            .unwrap_or(&format!("{:?}", thread::current().id())),
        flexi_logger::style(level, level),
        record.module_path().unwrap_or("<unnamed>"),
        record.args(),
    )
}

fn thread_fun(
    rx: Receiver<()>,
    gpu_stealing: bool,
    priv_replica_infos: &BTreeMap<SectorId, PrivateReplicaInfo>,
    candidates: &[Candidate],
) -> RunInfo {
    let timing = Instant::now();
    let mut iteration = 0;
    while iteration < std::u8::MAX {
        info!("high iter {}", iteration);

        // This is the higher priority proof, get it on the GPU even if there is one running
        // already there
        if gpu_stealing {
            let gpu_lock = gpu::acquire_gpu().unwrap();
            info!("Trying to acquire GPU lock");
            while !gpu::gpu_is_available().unwrap_or(false) {
                thread::sleep(Duration::from_millis(100));
                trace!("Trying to acquire GPU lock");
            }
            debug!("Acquired GPU lock, dropping it again");
            gpu::drop_acquire_lock(gpu_lock);
        }

        // Run the actual proof
        election_post::do_generate_post(&priv_replica_infos, &candidates);

        // Waiting for this thread to be killed
        match rx.try_recv() {
            Ok(_) | Err(TryRecvError::Disconnected) => {
                debug!("High priority proofs received kill message");
                break;
            }
            Err(TryRecvError::Empty) => (),
        }
        iteration += 1;
    }
    RunInfo {
        elapsed: timing.elapsed(),
        iterations: iteration,
    }
}

fn spawn_thread(
    name: &str,
    gpu_stealing: bool,
    priv_replica_infos: BTreeMap<SectorId, PrivateReplicaInfo>,
    candidates: Vec<Candidate>,
) -> (Sender<()>, thread::JoinHandle<RunInfo>) {
    let (tx, rx) = mpsc::channel();

    let thread_config = thread::Builder::new().name(name.to_string());
    let handler = thread_config
        .spawn(move || -> RunInfo {
            thread_fun(rx, gpu_stealing, &priv_replica_infos, &candidates)
        })
        .expect("Could not spawn thread");

    (tx, handler)
}

fn main() {
    flexi_logger::Logger::with_env()
        .format(colored_with_thread)
        .start()
        .expect("Initializing logger failed. Was another logger already initialized?");

    let matches = App::new("gpu-cpu-test")
        .version("0.1")
        .about("Tests if moving proofs from GPU to CPU works")
        .arg(
            Arg::with_name("parallel")
                .long("parallel")
                .help("Run proofs in parallel.")
                .default_value("true"),
        )
        .arg(
            Arg::with_name("gpu-stealing")
                .long("gpu-stealing")
                .help("Force high priority proof on the GPU and let low priority one continue on CPU.")
                .default_value("true"),
        )
        .get_matches();

    let parallel = value_t!(matches, "parallel", bool).unwrap();
    if parallel {
        info!("Running high and low priority proofs in parallel")
    } else {
        info!("Running high priority proofs only")
    }
    let gpu_stealing = value_t!(matches, "gpu-stealing", bool).unwrap();
    if gpu_stealing {
        info!("Force low piority proofs to CPU")
    } else {
        info!("Let everyone queue up to run on GPU")
    }

    // All channels we send a termination message to
    let mut senders = Vec::new();
    // All thread handles that get terminated
    let mut threads: Vec<Option<thread::JoinHandle<_>>> = Vec::new();

    // Create fixtures only once for both threads
    let priv_replica_info = election_post::generate_priv_replica_info_fixture();
    let candidates = election_post::generate_candidates_fixture(&priv_replica_info);

    // Put each proof into it's own scope (the other one is due to the if statement)
    {
        let (tx, handler) = spawn_thread(
            "high",
            gpu_stealing,
            priv_replica_info.clone(),
            candidates.clone(),
        );
        senders.push(tx);
        threads.push(Some(handler));
    }

    if parallel {
        let (tx, handler) = spawn_thread("low", false, priv_replica_info, candidates);
        senders.push(tx);
        threads.push(Some(handler));
    }

    // Terminate all threads after that amount of time
    let timeout = Duration::from_secs(TIMEOUT);
    thread::sleep(timeout);
    info!("Waited long enough to kill all threads");
    for tx in senders {
        tx.send(()).unwrap();
    }

    for thread in &mut threads {
        if let Some(handler) = thread.take() {
            let thread_name = handler
                .thread()
                .name()
                .unwrap_or(&format!("{:?}", handler.thread().id()))
                .to_string();
            let run_info = handler.join().unwrap();
            info!("Thread {} info: {:?}", thread_name, run_info);
        }
    }
}
