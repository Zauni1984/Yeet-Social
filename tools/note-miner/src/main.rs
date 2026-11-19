//! Solo miner for the Note chain (scrypt 1024/1/1), talking to the project's
//! own node over JSON-RPC (getblocktemplate / submitblock). CPU only.
//!
//! The node builds the block rules (reward, difficulty, transactions); this
//! program only assembles the block from the template and searches a nonce.
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

/// Appends a UTC-stamped line to miner.log next to the program, so a refusal
/// by the node can still be read after the window has been closed. A message
/// that just repeats is written at most once every five minutes.
fn log_line(msg: &str) {
    use std::io::Write;
    use std::sync::Mutex;
    static LAST: Mutex<Option<(String, Instant)>> = Mutex::new(None);
    {
        let mut last = LAST.lock().unwrap();
        if let Some((m, t)) = last.as_ref() {
            if m == msg && t.elapsed() < Duration::from_secs(300) { return; }
        }
        *last = Some((msg.to_string(), Instant::now()));
    }
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let (d, r) = (secs / 86400, secs % 86400);
    // civil-from-days (Howard Hinnant)
    let z = d as i64 + 719468; let era = z.div_euclid(146097); let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153; let day = doy - (153 * mp + 2) / 5 + 1; let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + if month <= 2 { 1 } else { 0 };
    let line = format!("{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02} UTC  {msg}\n", r / 3600, r % 3600 / 60, r % 60);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open("miner.log") { let _ = f.write_all(line.as_bytes()); }
}

fn sha256d(d: &[u8]) -> [u8; 32] {
    let a = Sha256::digest(d);
    let b = Sha256::digest(a);
    b.into()
}

/// scrypt(N=1024, r=1, p=1, 32 bytes) with the header as password and salt.
fn pow_hash(header: &[u8; 80]) -> [u8; 32] {
    let params = scrypt::Params::new(10, 1, 1, 32).expect("params");
    let mut out = [0u8; 32];
    scrypt::scrypt(header, header, &params, &mut out).expect("scrypt");
    out
}

fn varint(n: u64) -> Vec<u8> {
    match n {
        0..=0xfc => vec![n as u8],
        0xfd..=0xffff => { let mut v = vec![0xfd]; v.extend(&(n as u16).to_le_bytes()); v }
        0x1_0000..=0xffff_ffff => { let mut v = vec![0xfe]; v.extend(&(n as u32).to_le_bytes()); v }
        _ => { let mut v = vec![0xff]; v.extend(&n.to_le_bytes()); v }
    }
}

/// Minimal script number push (BIP34 block height).
fn push_height(h: u64) -> Vec<u8> {
    let mut n = h;
    let mut b = Vec::new();
    while n > 0 { b.push((n & 0xff) as u8); n >>= 8; }
    if b.last().map_or(false, |x| x & 0x80 != 0) { b.push(0); }
    let mut v = vec![b.len() as u8];
    v.extend(b);
    v
}

fn rev_hex(s: &str) -> Result<Vec<u8>, String> {
    let mut v = hex::decode(s).map_err(|e| format!("bad hex: {e}"))?;
    v.reverse();
    Ok(v)
}

fn merkle_root(mut level: Vec<[u8; 32]>) -> [u8; 32] {
    while level.len() > 1 {
        if level.len() % 2 == 1 { level.push(*level.last().unwrap()); }
        level = level.chunks(2).map(|p| { let mut c = [0u8; 64]; c[..32].copy_from_slice(&p[0]); c[32..].copy_from_slice(&p[1]); sha256d(&c) }).collect();
    }
    level[0]
}

/// hash (little-endian number, as produced) <= target (big-endian bytes)
fn meets(hash_le: &[u8; 32], target_be: &[u8; 32]) -> bool {
    for i in 0..32 {
        let h = hash_le[31 - i];
        if h < target_be[i] { return true; }
        if h > target_be[i] { return false; }
    }
    true
}

struct Rpc { url: String, auth: String }
impl Rpc {
    fn call(&self, method: &str, params: Value) -> Result<Value, String> {
        let body = json!({"jsonrpc":"1.0","id":"note-miner","method":method,"params":params});
        let resp = ureq::post(&self.url).set("Authorization", &self.auth).timeout(Duration::from_secs(60)).send_json(body);
        let v: Value = match resp {
            Ok(r) => r.into_json().map_err(|e| format!("{method}: bad reply: {e}"))?,
            Err(ureq::Error::Status(code, r)) => {
                let t = r.into_string().unwrap_or_default();
                match serde_json::from_str::<Value>(&t) { Ok(v) => v, Err(_) => return Err(format!("{method}: HTTP {code} {t}")) }
            }
            Err(e) => return Err(format!("{method}: {e}")),
        };
        if !v["error"].is_null() { return Err(format!("{method}: {}", v["error"])); }
        Ok(v["result"].clone())
    }
}

#[derive(Clone)]
struct Job {
    id: u64,
    version: i32,
    prev: Vec<u8>,          // header byte order
    time: u32,
    bits: u32,
    target_be: [u8; 32],
    height: u64,
    value: u64,
    payout_script: Vec<u8>,
    commitment: Option<Vec<u8>>,
    tx_ids: Vec<[u8; 32]>,  // internal byte order
    tx_data: Vec<Vec<u8>>,
}

impl Job {
    /// (coinbase without witness, coinbase as serialized in the block)
    fn coinbase(&self, extranonce: u64) -> (Vec<u8>, Vec<u8>) {
        let mut script_sig = push_height(self.height);
        script_sig.push(8);
        script_sig.extend(&extranonce.to_le_bytes());
        script_sig.extend(b"\x04Yeet");
        let mut ins = Vec::new();
        ins.extend([0u8; 32]);
        ins.extend(&0xffff_ffffu32.to_le_bytes());
        ins.extend(varint(script_sig.len() as u64));
        ins.extend(&script_sig);
        ins.extend(&0xffff_ffffu32.to_le_bytes());
        let mut outs = Vec::new();
        let n_out = if self.commitment.is_some() { 2 } else { 1 };
        outs.extend(varint(n_out));
        outs.extend(&self.value.to_le_bytes());
        outs.extend(varint(self.payout_script.len() as u64));
        outs.extend(&self.payout_script);
        if let Some(c) = &self.commitment {
            outs.extend(&0u64.to_le_bytes());
            outs.extend(varint(c.len() as u64));
            outs.extend(c);
        }
        let mut plain = Vec::new();
        plain.extend(&1i32.to_le_bytes());
        plain.push(1);
        plain.extend(&ins);
        plain.extend(&outs);
        plain.extend(&0u32.to_le_bytes());
        let full = if self.commitment.is_some() {
            let mut f = Vec::new();
            f.extend(&1i32.to_le_bytes());
            f.extend([0x00, 0x01]);          // segwit marker + flag
            f.push(1);
            f.extend(&ins);
            f.extend(&outs);
            f.extend([0x01, 0x20]);          // one witness item of 32 bytes
            f.extend([0u8; 32]);             // witness reserved value
            f.extend(&0u32.to_le_bytes());
            f
        } else { plain.clone() };
        (plain, full)
    }

    fn header(&self, extranonce: u64) -> ([u8; 80], Vec<u8>) {
        let (plain, full) = self.coinbase(extranonce);
        let mut ids = vec![sha256d(&plain)];
        ids.extend(self.tx_ids.iter().cloned());
        let root = merkle_root(ids);
        let mut h = [0u8; 80];
        h[..4].copy_from_slice(&self.version.to_le_bytes());
        h[4..36].copy_from_slice(&self.prev);
        h[36..68].copy_from_slice(&root);
        h[68..72].copy_from_slice(&self.time.to_le_bytes());
        h[72..76].copy_from_slice(&self.bits.to_le_bytes());
        (h, full)
    }

    fn block_hex(&self, header: &[u8; 80], coinbase_full: &[u8]) -> String {
        let mut b = header.to_vec();
        b.extend(varint(1 + self.tx_data.len() as u64));
        b.extend(coinbase_full);
        for t in &self.tx_data { b.extend(t); }
        hex::encode(b)
    }
}

fn fetch_job(rpc: &Rpc, payout_script: &[u8], id: u64) -> Result<Job, String> {
    let t = rpc.call("getblocktemplate", json!([{"rules": ["segwit"]}]))?;
    let s = |k: &str| t[k].as_str().ok_or(format!("template has no {k}"));
    let mut target_be = [0u8; 32];
    target_be.copy_from_slice(&hex::decode(s("target")?).map_err(|e| e.to_string())?);
    let mut tx_ids = Vec::new();
    let mut tx_data = Vec::new();
    for tx in t["transactions"].as_array().cloned().unwrap_or_default() {
        let id_hex = tx["txid"].as_str().or(tx["hash"].as_str()).ok_or("tx without id")?;
        let mut a = [0u8; 32];
        a.copy_from_slice(&rev_hex(id_hex)?);
        tx_ids.push(a);
        tx_data.push(hex::decode(tx["data"].as_str().ok_or("tx without data")?).map_err(|e| e.to_string())?);
    }
    let commitment = match t["default_witness_commitment"].as_str() {
        Some(c) => Some(hex::decode(c).map_err(|e| e.to_string())?),
        None => None,
    };
    Ok(Job {
        id,
        version: t["version"].as_i64().ok_or("no version")? as i32,
        prev: rev_hex(s("previousblockhash")?)?,
        time: t["curtime"].as_u64().ok_or("no curtime")? as u32,
        bits: u32::from_str_radix(s("bits")?, 16).map_err(|e| e.to_string())?,
        target_be,
        height: t["height"].as_u64().ok_or("no height")?,
        value: t["coinbasevalue"].as_u64().ok_or("no coinbasevalue")?,
        payout_script: payout_script.to_vec(),
        commitment,
        tx_ids,
        tx_data,
    })
}

fn read_conf(path: &str) -> HashMap<String, String> {
    let mut m = HashMap::new();
    if let Ok(t) = std::fs::read_to_string(path) {
        for l in t.lines() {
            let l = l.trim();
            if l.is_empty() || l.starts_with('#') { continue; }
            if let Some((k, v)) = l.split_once('=') { m.insert(k.trim().to_string(), v.trim().to_string()); }
        }
    }
    m
}

fn self_test() -> Result<(), String> {
    // Litecoin genesis block: same header layout and the same scrypt PoW.
    let mut h = [0u8; 80];
    h[..4].copy_from_slice(&1i32.to_le_bytes());
    h[36..68].copy_from_slice(&rev_hex("97ddfbbae6be97fd6cdf3e7ca13232a3afff2353e29badfab7f73011edd4ced9")?);
    h[68..72].copy_from_slice(&1317972665u32.to_le_bytes());
    h[72..76].copy_from_slice(&0x1e0ffff0u32.to_le_bytes());
    h[76..80].copy_from_slice(&2084524493u32.to_le_bytes());
    let mut id = sha256d(&h); id.reverse();
    if hex::encode(id) != "12a765e31ffd4059bada1e25190f6e98c99d9714d334efa41a195a7e7e04bfe2" { return Err("self-test: header hash mismatch".into()); }
    let mut target = [0u8; 32];
    target[2] = 0x0f; target[3] = 0xff; target[4] = 0xf0; // 0x1e0ffff0
    if !meets(&pow_hash(&h), &target) { return Err("self-test: scrypt proof of work mismatch".into()); }
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        log_line(&format!("stopped with error: {e}"));
        eprintln!("\nERROR: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    self_test()?;
    if args.iter().any(|a| a == "--self-test") { println!("self-test ok"); return Ok(()); }
    if args.iter().any(|a| a == "--bench") {
        let (mut h, t, mut n) = ([7u8; 80], Instant::now(), 0u32);
        while t.elapsed() < Duration::from_secs(4) { h[76..80].copy_from_slice(&n.to_le_bytes()); pow_hash(&h); n += 1; }
        println!("{:.1} kH/s per thread", n as f64 / t.elapsed().as_secs_f64() / 1000.0);
        return Ok(());
    }
    let conf_path = args.get(1).cloned().unwrap_or_else(|| "miner.conf".into());
    let c = read_conf(&conf_path);
    let get = |k: &str| c.get(k).cloned().ok_or(format!("{conf_path}: missing {k}"));
    let rpc = Arc::new(Rpc {
        url: get("rpc_url")?,
        auth: format!("Basic {}", base64::Engine::encode(&base64::engine::general_purpose::STANDARD, format!("{}:{}", get("rpc_user")?, get("rpc_password")?))),
    });
    let max_blocks: u64 = c.get("blocks").and_then(|v| v.parse().ok()).unwrap_or(0);
    let threads: usize = c.get("threads").and_then(|v| v.parse().ok()).filter(|n| *n > 0)
        .unwrap_or_else(|| std::thread::available_parallelism().map(|n| n.get().saturating_sub(1).max(1)).unwrap_or(4)); // leave one core for the desktop

    println!("Note miner - node {}", rpc.url);
    let info = rpc.call("getblockchaininfo", json!([]))?;
    println!("connected: chain {} at height {}, difficulty {}", info["chain"], info["blocks"], info["difficulty"]);
    let address = match c.get("address") { Some(a) if !a.is_empty() => a.clone(), _ => rpc.call("getnewaddress", json!([]))?.as_str().unwrap_or_default().to_string() };
    let va = rpc.call("validateaddress", json!([address]))?;
    if va["isvalid"].as_bool() != Some(true) { return Err(format!("address {address} is not valid")); }
    let payout_script = hex::decode(va["scriptPubKey"].as_str().ok_or("no scriptPubKey")?).map_err(|e| e.to_string())?;
    log_line(&format!("started: height {}, difficulty {}, {threads} thread(s)", info["blocks"], info["difficulty"]));
    println!("rewards go to {address}; {threads} thread(s); {}", if max_blocks > 0 { format!("stopping after {max_blocks} block(s)") } else { "running until closed".into() });

    let job = Arc::new(RwLock::new(fetch_job(&rpc, &payout_script, 1)?));
    // Ask the node to check our block layout once (no proof of work needed).
    {
        let j = job.read().unwrap().clone();
        let (h, cb) = j.header(0);
        match rpc.call("getblocktemplate", json!([{"mode": "proposal", "data": j.block_hex(&h, &cb), "rules": ["segwit"]}])) {
            Ok(Value::Null) => println!("block layout accepted by the node (height {}, reward {} base units)", j.height, j.value),
            Ok(v) => return Err(format!("the node rejects our block layout: {v}")),
            Err(e) => println!("note: layout pre-check not available ({e})"),
        }
    }

    let hashes = Arc::new(AtomicU64::new(0));
    let found = Arc::new(AtomicU64::new(0));
    let stop = Arc::new(AtomicBool::new(false));
    // Id of a template that has been solved: everyone idles until new work arrives.
    let solved = Arc::new(AtomicU64::new(0));
    for t in 0..threads {
        let (job, hashes, found, stop, rpc, solved) = (job.clone(), hashes.clone(), found.clone(), stop.clone(), rpc.clone(), solved.clone());
        std::thread::spawn(move || {
            let mut seen = 0u64;
            let mut extranonce = ((t as u64) << 40) | (std::process::id() as u64) << 8;
            loop {
                if stop.load(Ordering::Relaxed) { return; }
                let j = job.read().unwrap().clone();
                if solved.load(Ordering::SeqCst) == j.id { std::thread::sleep(Duration::from_millis(100)); continue; }
                if j.id == seen { extranonce = extranonce.wrapping_add(1); }
                seen = j.id;
                let (mut h, cb) = j.header(extranonce);
                let mut nonce: u32 = 0;
                loop {
                    h[76..80].copy_from_slice(&nonce.to_le_bytes());
                    if meets(&pow_hash(&h), &j.target_be) {
                        if solved.swap(j.id, Ordering::SeqCst) == j.id { break; } // another thread was first
                        let hex_block = j.block_hex(&h, &cb);
                        match rpc.call("submitblock", json!([hex_block])) {
                            Ok(Value::Null) => { let n = found.fetch_add(1, Ordering::SeqCst) + 1; println!("\n*** BLOCK FOUND: height {} accepted (block {n} of this session) ***", j.height); log_line(&format!("block found: height {} accepted", j.height)); }
                            Ok(v) => { println!("\nblock at height {} not accepted: {v}", j.height); log_line(&format!("block at height {} NOT accepted: {v}", j.height)); }
                            Err(e) => { println!("\nsubmit failed: {e}"); log_line(&format!("submit failed: {e}")); }
                        }
                        break;
                    }
                    nonce = nonce.wrapping_add(1);
                    if nonce & 0x3f == 0 {
                        hashes.fetch_add(64, Ordering::Relaxed);
                        if stop.load(Ordering::Relaxed) { return; }
                        if job.read().unwrap().id != seen || solved.load(Ordering::Relaxed) == seen { break; }
                    }
                    if nonce == 0 { break; } // nonce space used up: new extranonce
                }
            }
        });
    }

    let start = Instant::now();
    let (mut last_h, mut last_t, mut last_found, mut id) = (0u64, Instant::now(), 0u64, 1u64);
    let mut last_template = Instant::now();
    loop {
        for _ in 0..25 { std::thread::sleep(Duration::from_millis(200)); if solved.load(Ordering::SeqCst) == id { break; } }
        let f = found.load(Ordering::SeqCst);
        let tip_changed = match rpc.call("getbestblockhash", json!([])) {
            Ok(v) => rev_hex(v.as_str().unwrap_or("")).map(|p| p != job.read().unwrap().prev).unwrap_or(false),
            Err(e) => { println!("\nnode not reachable: {e} (is the tunnel window still open?)"); log_line(&format!("node not reachable: {e}")); false }
        };
        if f != last_found || tip_changed || solved.load(Ordering::SeqCst) == id || last_template.elapsed() > Duration::from_secs(30) {
            id += 1;
            match fetch_job(&rpc, &payout_script, id) {
                Ok(j) => { if tip_changed || f != last_found { println!("\nnew work: height {}, difficulty target {}...", j.height, &hex::encode(j.target_be)[..16]); } *job.write().unwrap() = j; last_template = Instant::now(); }
                Err(e) => { println!("\ncould not get new work: {e}"); log_line(&format!("could not get new work: {e}")); }
            }
            last_found = f;
        }
        let h = hashes.load(Ordering::Relaxed);
        let rate = (h - last_h) as f64 / last_t.elapsed().as_secs_f64();
        last_h = h; last_t = Instant::now();
        print!("\r{:>8.1} kH/s   total {:>7.1} M hashes   blocks {}   running {} min   ", rate / 1000.0, h as f64 / 1e6, f, start.elapsed().as_secs() / 60);
        use std::io::Write; let _ = std::io::stdout().flush();
        if max_blocks > 0 && f >= max_blocks { stop.store(true, Ordering::SeqCst); println!("\ndone: {f} block(s) mined."); return Ok(()); }
    }
}
