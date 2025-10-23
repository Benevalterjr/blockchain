use axum::{routing::get, Router, Json};
use serde::{Serialize, Deserialize};
use shuttle_axum::ShuttleAxum;
use shuttle_runtime::main;
use std::sync::{Arc, Mutex};
use rand::Rng;
use std::time::Instant;

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
// Funções de primalidade
// ----------------------
fn is_prime(n: u64) -> bool {
    if n < 2 { return false; }
    for i in 2..=((n as f64).sqrt() as u64) {
        if n % i == 0 { return false; }
    }
    true
}

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
        if x == 1 || x == n - 1 {
            continue;
        }
        for _ in 0..r - 1 {
            x = mod_pow(x, 2, n);
            if x == n - 1 {
                continue 'outer;
            }
        }
        return false;
    }
    true
}

fn mod_pow(mut base: u64, mut exp: u64, modu: u64) -> u64 {
    let mut result = 1;
    base %= modu;
    while exp > 0 {
        if exp % 2 == 1 {
            result = (result * base) % modu;
        }
        base = (base * base) % modu;
        exp /= 2;
    }
    result
}

// ----------------------
// Mineração simples
// ----------------------
fn mine_block(prev: &Block, min_digits: u32) -> Block {
    let mut rng = rand::thread_rng();
    loop {
        let a = rng.gen_range(10_u64.pow(min_digits - 1)..10_u64.pow(min_digits));
        let b = rng.gen_range(1..1000);
        let c = rng.gen_range(10_u64.pow(min_digits - 1)..10_u64.pow(min_digits));
        let d = rng.gen_range(1..1000);
        let n = a * d + b * c;

        if miller_rabin(n, 12) {
            let hash = format!("{:x}", n ^ prev.prime);
            return Block {
                index: prev.index + 1,
                prev_hash: prev.hash.clone(),
                prime: n,
                a, b, c, d,
                hash,
            };
        }
    }
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
        a: 1, b: 1, c: 1, d: 1,
        hash: "genesis".into(),
    };
    let chain = Arc::new(Mutex::new(vec![genesis]));

    let router = Router::new()
        .route("/", get(|| async { "🚀 Proof-of-Prime Blockchain Node" }))
        .route("/mine", get({
            let chain = chain.clone();
            move || async move {
                let mut chain = chain.lock().unwrap();
                let start = Instant::now();
                let new_block = mine_block(&chain.last().unwrap(), 7);
                let duration = start.elapsed().as_secs_f64();
                chain.push(new_block.clone());
                Json(serde_json::json!({
                    "index": new_block.index,
                    "prime": new_block.prime,
                    "duration": duration,
                    "height": chain.len(),
                }))
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
