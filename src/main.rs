use axum::{routing::get, Router, Json};
use serde::{Deserialize, Serialize};
use shuttle_axum::ShuttleAxum;
use std::sync::{Arc, Mutex, atomic::{AtomicU64, AtomicU32, Ordering}};
use rand::Rng;
use std::time::Instant;
use tokio::task;
use tokio::sync::mpsc;
use num::Integer;
use lazy_static::lazy_static;
use log::{info, debug};
use env_logger;

// ----------------------
// Lazy Static
// ----------------------
lazy_static! {
    static ref N_LIMIT: AtomicU64 = AtomicU64::new(1000);
    static ref MIN_DIGITS: AtomicU32 = AtomicU32::new(7);
    static ref MIN_PROB: AtomicU64 = AtomicU64::new(100); // 0.01
}

const TARGET_TIME: f64 = 10.0;

// ----------------------
// Block
// ----------------------
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Block {
    index: u64,
    prev_hash: String,
    prime: u64,
    a: u64,
    b: u64,
    c: u64,
    d: u64,
    hash: String,
}

// ----------------------
// Stats
// ----------------------
#[derive(Debug, Clone, Serialize)]
struct MiningStats {
    candidates: u64,
    gcd_rejected: u64,
    heuristic_rejected: u64,
    miller_rabin_rejected: u64,
    probability: f64,
}

// ----------------------
// Miller-Rabin
// ----------------------
fn miller_rabin(n: u64, k: u32) -> bool {
    if n <= 1 { return false; }
    if n <= 3 { return true; }
    if n % 2 == 0 { return false; }

    let mut d = n - 1;
    let mut r = 0;
    while d % 2 == 0 {
        d /= 2;
        r += 1;
    }

    let mut rng = rand::thread_rng();
    'outer: for _ in 0..k {
        let a = rng.gen_range(2..n - 1);
        let mut x = mod_pow(a, d, n);
        if x == 1 || x == n - 1 { continue; }
        for _ in 0..r - 1 {
            x = mod_pow(x, 2, n);
            if x == n - 1 { continue 'outer; }
        }
        return false;
    }
    true
}

fn mod_pow(mut base: u64, mut exp: u64, modu: u64) -> u64 {
    let mut result = 1;
    base %= modu;
    while exp > 0 {
        if exp % 2 == 1 { result = (result * base) % modu; }
        base = (base * base) % modu;
        exp /= 2;
    }
    result
}

// ----------------------
// Heurística
// ----------------------
fn prime_heuristic(n: u64, min_prob: f64) -> bool {
    if n < 2 { return false; }
    let ln_n = (n as f64).ln();
    1.0 / ln_n >= min_prob
}

// ----------------------
// Minerador
// ----------------------
fn mine_worker(prev: &Block) -> (Block, MiningStats) {
    let mut rng = rand::thread_rng();
    let mut stats = MiningStats {
        candidates: 0,
        gcd_rejected: 0,
        heuristic_rejected: 0,
        miller_rabin_rejected: 0,
        probability: 0.0,
    };

    let n_limit = N_LIMIT.load(Ordering::Relaxed);
    let min_digits = MIN_DIGITS.load(Ordering::Relaxed);
    let min_prob = MIN_PROB.load(Ordering::Relaxed) as f64 / 10000.0;

    loop {
        stats.candidates += 1;

        let a = rng.gen_range(10_u64.pow(min_digits - 1)..10_u64.pow(min_digits));
        let b = rng.gen_range(1..=n_limit);
        let c = rng.gen_range(10_u64.pow(min_digits - 1)..10_u64.pow(min_digits));
        let d = rng.gen_range(1..=n_limit);

        if a.gcd(&b) != 1 || c.gcd(&d) != 1 {
            stats.gcd_rejected += 1;
            continue;
        }

        let n = a * d + b * c;

        if !prime_heuristic(n, min_prob) {
            stats.heuristic_rejected += 1;
            continue;
        }

        if miller_rabin(n, 12) {
            let hash = format!("{:x}", n ^ prev.prime);
            stats.probability = 1.0 / (n as f64).ln();

            let block = Block {
                index: prev.index + 1,
                prev_hash: prev.hash.clone(),
                prime: n,
                a, b, c, d,
                hash,
            };

            info!("Bloco minerado! Primo: {} ({} dígitos)", n, n.to_string().len());
            return (block, stats);
        }
        stats.miller_rabin_rejected += 1;
    }
}

// ----------------------
// Mineração paralela
// ----------------------
async fn mine_block_parallel(prev: Block, workers: usize) -> (Block, MiningStats) {
    let (tx, mut rx) = mpsc::channel::<(Block, MiningStats)>(1);
    let prev = Arc::new(prev);

    for _ in 0..workers {
        let tx = tx.clone();
        let prev = prev.clone();
        task::spawn_blocking(move || {
            let result = mine_worker(&prev);
            let _ = tx.blocking_send(result);
        });
    }

    rx.recv().await.expect("Falha na mineração")
}

// ----------------------
// Ajuste de dificuldade
// ----------------------
fn adjust_difficulty(duration: f64) {
    if duration < TARGET_TIME * 0.6 {
        N_LIMIT.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |x| Some((x as f64 * 1.5) as u64)).ok();
        MIN_DIGITS.fetch_add(1, Ordering::Relaxed);
        MIN_PROB.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |x| Some((x as f64 * 1.2).min(1000.0) as u64)).ok();
        info!("Dificuldade aumentada! n_limit: {}", N_LIMIT.load(Ordering::Relaxed));
    } else if duration > TARGET_TIME * 1.4 {
        N_LIMIT.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |x| Some((x as f64 * 0.7).max(100.0) as u64)).ok();
        MIN_PROB.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |x| Some((x as f64 * 0.8).max(50.0) as u64)).ok();
        info!("Dificuldade reduzida! n_limit: {}", N_LIMIT.load(Ordering::Relaxed));
    }
}

// ----------------------
// Entry point
// ----------------------
#[shuttle_runtime::main]
async fn axum() -> ShuttleAxum {
    env_logger::init();

    let genesis = Block {
        index: 0,
        prev_hash: "0".into(),
        prime: 2,
        a: 1, b: 1, c: 1, d: 1,
        hash: "genesis".into(),
    };

    let chain = Arc::new(Mutex::new(vec![genesis]));

    let router = Router::new()
        .route("/", get(|| async { "Proof-of-Prime Blockchain Node" }))

        .route("/mine", get({
            let chain = chain.clone();
            move || async move {
                let last_block = {
                    let guard = chain.lock().unwrap();
                    guard.last().unwrap().clone()
                };

                let start = Instant::now();
                let (new_block, stats) = mine_block_parallel(last_block, 4).await;
                let duration = start.elapsed().as_secs_f64();

                {
                    let mut guard = chain.lock().unwrap();
                    guard.push(new_block.clone());
                }

                adjust_difficulty(duration);

                let height = chain.lock().unwrap().len();

                Json(serde_json::json!({
                    "index": new_block.index,
                    "prime": new_block.prime,
                    "digits": new_block.prime.to_string().len(),
                    "duration": format!("{:.3}s", duration),
                    "height": height,
                    "stats": {
                        "candidates": stats.candidates,
                        "gcd_rejected": stats.gcd_rejected,
                        "heuristic_rejected": stats.heuristic_rejected,
                        "miller_rabin_rejected": stats.miller_rabin_rejected,
                        "probability": format!("{:.5}", stats.probability)
                    },
                    "difficulty": {
                        "n_limit": N_LIMIT.load(Ordering::Relaxed),
                        "min_digits": MIN_DIGITS.load(Ordering::Relaxed),
                        "min_prob": format!("{:.4}", MIN_PROB.load(Ordering::Relaxed) as f64 / 10000.0)
                    }
                }))
            }
        }))

        .route("/chain", get({
            let chain = chain.clone();
            move || async move {
                let guard = chain.lock().unwrap();
                Json(guard.clone())
            }
        }));

    Ok(router.into())
}
