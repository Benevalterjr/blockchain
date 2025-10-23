use axum::{routing::get, Router, Json};
use serde::{Serialize, Deserialize};
use shuttle_axum::ShuttleAxum;
use std::sync::{Arc, Mutex};
use rand::Rng;
use std::time::Instant;
use tokio::task;
use std::f64;

// ----------------------
// Estrutura de bloco
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
// Miller-Rabin probabilístico
// ----------------------
fn miller_rabin(n: u64, k: u32) -> bool {
    if n < 2 { return false; }
    if n % 2 == 0 { return n == 2; }

    let mut d = n - 1;
    let mut r = 0;
    while d % 2 == 0 {
        d /= 2;
        r += 1;
    }

    let mut rng = rand::thread_rng();
    'outer: for _ in 0..k {
        let a = rng.gen_range(2..n - 2);
        let mut x = mod_pow(a, d, n);
        if x == 1 || x == n - 1 { continue; }
        for _ in 0..r-1 {
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
// Heurística 1/ln(N) + rejeição primos pequenos
// ----------------------
fn prime_heuristic(n: u64, small_primes: &[u64], min_prob: f64) -> bool {
    if n < 2 { return false; }
    for &p in small_primes {
        if n % p == 0 { return false; }
    }
    (1.0 / (n as f64).ln()) >= min_prob
}

// ----------------------
// Mineração de um bloco (usada por cada minerador)
// ----------------------
fn mine_block_worker(prev: &Block, min_digits: u32, n_limit: u64, small_primes: &[u64], min_prob: f64) -> (Block, u64, u64, u64, u64) {
    let mut rng = rand::thread_rng();
    let mut cand = 0;
    let mut gcd_fail = 0;
    let mut heur_fail = 0;
    let mut mr_fail = 0;

    loop {
        cand += 1;
        let a = rng.gen_range(10_u64.pow(min_digits - 1)..10_u64.pow(min_digits));
        let b = rng.gen_range(1..=n_limit);
        let c = rng.gen_range(10_u64.pow(min_digits - 1)..10_u64.pow(min_digits));
        let d = rng.gen_range(1..=n_limit);

        if gcd(a,b) != 1 || gcd(c,d) != 1 {
            gcd_fail += 1;
            continue;
        }

        let n = a*d + b*c;

        if !prime_heuristic(n, small_primes, min_prob) {
            heur_fail += 1;
            continue;
        }

        if miller_rabin(n, 12) {
            let hash = format!("{:x}", n ^ prev.prime);
            return (Block {
                index: prev.index + 1,
                prev_hash: prev.hash.clone(),
                prime: n,
                a, b, c, d,
                hash,
            }, cand, gcd_fail, heur_fail, mr_fail);
        } else {
            mr_fail += 1;
        }
    }
}

fn gcd(a: u64, b: u64) -> u64 {
    let mut a = a;
    let mut b = b;
    while b != 0 {
        let tmp = b;
        b = a % b;
        a = tmp;
    }
    a
}

// ----------------------
// Minerador paralelo usando Tokio
// ----------------------
async fn mine_block_parallel(prev: Block, min_digits: u32, n_limit: u64, small_primes: Vec<u64>, min_prob: f64, miners: usize) -> (Block, u64, u64, u64, u64) {
    let mut handles = Vec::new();
    for _ in 0..miners {
        let prev_clone = prev.clone();
        let sp = small_primes.clone();
        handles.push(task::spawn_blocking(move || {
            mine_block_worker(&prev_clone, min_digits, n_limit, &sp, min_prob)
        }));
    }

    let (res, _, _, _, _) = futures::future::select_all(handles).await.0.unwrap();
    res
}

// ----------------------
// Estado global da blockchain
// ----------------------
#[shuttle_runtime::main]
async fn axum() -> ShuttleAxum {
    let genesis = Block {
        index: 0,
        prev_hash: "0".into(),
        prime: 2,
        a:1, b:1, c:1, d:1,
        hash:"genesis".into(),
    };
    let chain = Arc::new(Mutex::new(vec![genesis]));

    let router = Router::new()
        .route("/", get(|| async { "🚀 Proof-of-Prime Blockchain Node" }))
        .route("/mine", get({
            let chain = chain.clone();
            move || {
                let chain = chain.clone();
                async move {
                    let mut chain = chain.lock().unwrap();
                    let start = Instant::now();

                    let small_primes = vec![2,3,5,7,11,13,17,19,23,29,31,37,41,43,47];
                    let n_limit = 50_000;
                    let min_digits = 7;
                    let min_prob = 0.01;
                    let miners = 4;

                    let new_block = mine_block_parallel(chain.last().unwrap().clone(), min_digits, n_limit, small_primes, min_prob, miners).await;
                    let duration = start.elapsed().as_secs_f64();

                    chain.push(new_block.clone());

                    Json(serde_json::json!({
                        "index": new_block.index,
                        "prime": new_block.prime,
                        "duration": duration,
                        "height": chain.len()
                    }))
                }
            }
        }))
        .route("/chain", get({
            let chain = chain.clone();
            move || async move {
                let chain = chain.lock().unwrap();
                Json(&*chain)
            }
        }));

    Ok(router.into())
}
