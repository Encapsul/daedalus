//! daedalus-fuzz — simplified high-throughput fuzzer for daedalus.

#![forbid(unsafe_code)]

use anyhow::Result;
use arbitrary::Unstructured;

use crate::cli_fuzz::CliFuzzTarget;
use crate::crypto_fuzz::CryptoFuzzTarget;
use crate::format_fuzz::FormatFuzzTarget;
use crate::sisr_fuzz::SisrManifestFuzzTarget;
use crate::stub_fuzz::StubFuzzTarget;
use rand::Rng;
use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tokio::time::interval;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CorpusEntry {
    pub data: Vec<u8>,
    pub coverage_hash: u64,
    pub timestamp: u64,
}

impl CorpusEntry {
    pub fn new(data: Vec<u8>) -> Self {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        data.hash(&mut hasher);
        Self {
            data,
            coverage_hash: hasher.finish(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CrashCase {
    pub input: Vec<u8>,
    pub error_message: String,
    pub minimized: bool,
    pub original_size: usize,
}

#[derive(Debug, Default)]
pub struct GlobalStats {
    pub iterations: AtomicU64,
    pub crashes: AtomicU64,
    pub panics: AtomicU64,
    pub timeouts: AtomicU64,
    pub corpus_size: AtomicU64,
    pub unique_crashes: std::sync::Mutex<HashMap<String, u64>>,
}

impl GlobalStats {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn record_iteration(&self) {
        self.iterations.fetch_add(1, Ordering::Relaxed);
    }
    pub fn record_crash(&self, sig: &str) {
        self.crashes.fetch_add(1, Ordering::Relaxed);
        let mut m = self.unique_crashes.lock().unwrap();
        *m.entry(sig.to_string()).or_insert(0) += 1;
    }
    pub fn record_panic(&self) {
        self.panics.fetch_add(1, Ordering::Relaxed);
    }
    pub fn record_timeout(&self) {
        self.timeouts.fetch_add(1, Ordering::Relaxed);
    }
    pub fn record_corpus(&self) {
        self.corpus_size.fetch_add(1, Ordering::Relaxed);
    }
    pub fn record_unique_crash(&self, sig: String) {
        let mut m = self.unique_crashes.lock().unwrap();
        *m.entry(sig).or_insert(0) += 1;
    }
    pub fn snapshot(&self) -> FuzzStats {
        FuzzStats {
            iterations: self.iterations.load(Ordering::Relaxed),
            crashes: self.crashes.load(Ordering::Relaxed),
            panics: self.panics.load(Ordering::Relaxed),
            timeouts: self.timeouts.load(Ordering::Relaxed),
            unique_crashes: self.unique_crashes.lock().unwrap().clone(),
            corpus_size: self.corpus_size.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FuzzStats {
    pub iterations: u64,
    pub crashes: u64,
    pub panics: u64,
    pub timeouts: u64,
    pub unique_crashes: HashMap<String, u64>,
    pub corpus_size: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
pub enum MutationStrategy {
    BitFlip,
    ByteInsertDelete,
    Structured,
    Semantic,
    Crossover,
    #[default]
    Havoc,
}

#[derive(Debug, Clone)]
pub struct FuzzConfig {
    pub target_names: Vec<String>,
    pub iterations: Option<u64>,
    pub duration: Option<std::time::Duration>,
    pub workers: usize,
    pub corpus_dir: Option<std::path::PathBuf>,
    pub crash_dir: Option<std::path::PathBuf>,
    pub seed: u64,
    pub timeout_per_iteration: Duration,
    pub corpus_save_interval: Duration,
    pub stats_print_interval: Duration,
    pub mutation_strategy: MutationStrategy,
    pub max_input_size: usize,
    pub enable_cross_over: bool,
    pub minimize_crashes: bool,
}

impl Default for FuzzConfig {
    fn default() -> Self {
        Self {
            target_names: vec![
                "format".into(),
                "stub".into(),
                "cli".into(),
                "crypto".into(),
                "sisr".into(),
            ],
            iterations: None,
            duration: Some(Duration::from_secs(3600)),
            workers: num_cpus::get(),
            corpus_dir: None,
            crash_dir: None,
            seed: rand::random(),
            timeout_per_iteration: Duration::from_millis(5000),
            corpus_save_interval: Duration::from_secs(60),
            stats_print_interval: Duration::from_secs(10),
            mutation_strategy: MutationStrategy::Havoc,
            max_input_size: 10 * 1024 * 1024,
            enable_cross_over: true,
            minimize_crashes: true,
        }
    }
}

pub trait FuzzTarget: Send + Sync {
    fn name(&self) -> &'static str;
    fn generate_seed(&self, u: &mut Unstructured) -> Result<Vec<u8>>;
    fn mutate(&self, input: &[u8], u: &mut Unstructured) -> Result<Vec<u8>>;
    fn execute(&self, input: &[u8]) -> Result<()>;
    fn minimize(&self, input: &[u8]) -> Result<Vec<u8>> {
        Ok(input.to_vec())
    }
    fn is_valid(&self, input: &[u8]) -> bool {
        !input.is_empty()
    }
}

pub struct TargetRegistry {
    targets: std::collections::HashMap<&'static str, Arc<dyn FuzzTarget>>,
}

impl TargetRegistry {
    pub fn new() -> Self {
        let mut targets: std::collections::HashMap<&'static str, Arc<dyn FuzzTarget>> =
            std::collections::HashMap::new();
        targets.insert("format", Arc::new(FormatFuzzTarget));
        targets.insert("stub", Arc::new(StubFuzzTarget));
        targets.insert("cli", Arc::new(CliFuzzTarget));
        targets.insert("crypto", Arc::new(CryptoFuzzTarget));
        targets.insert("sisr", Arc::new(SisrManifestFuzzTarget));
        Self { targets }
    }
    pub fn get(&self, name: &str) -> Option<Arc<dyn FuzzTarget>> {
        self.targets.get(name).cloned()
    }
    pub fn all_names(&self) -> Vec<&'static str> {
        self.targets.keys().copied().collect()
    }
    pub fn all_targets(&self) -> Vec<Arc<dyn FuzzTarget>> {
        self.targets.values().cloned().collect()
    }
}

impl Default for TargetRegistry {
    fn default() -> Self {
        Self::new()
    }
}

pub struct FuzzHarness {
    config: FuzzConfig,
    registry: TargetRegistry,
    global_stats: Arc<GlobalStats>,
    corpus: Arc<Mutex<Vec<CorpusEntry>>>,
    crash_cases: Arc<Mutex<Vec<CrashCase>>>,
    start_time: Instant,
}

impl FuzzHarness {
    pub fn new(config: FuzzConfig) -> Self {
        let registry = TargetRegistry::new();
        let global_stats = Arc::new(GlobalStats::new());
        let corpus = Arc::new(Mutex::new(Vec::new()));
        let crash_cases = Arc::new(Mutex::new(Vec::new()));
        Self {
            config,
            registry,
            global_stats,
            corpus,
            crash_cases,
            start_time: Instant::now(),
        }
    }

    pub async fn load_corpus(&self) -> Result<()> {
        if let Some(dir) = &self.config.corpus_dir {
            if dir.exists() {
                let mut corpus = self.corpus.lock().await;
                for entry in std::fs::read_dir(dir)? {
                    let entry = entry?;
                    let path = entry.path();
                    if path.extension().is_some_and(|e| e == "bin") {
                        if let Ok(data) = std::fs::read(&path) {
                            if data.len() <= self.config.max_input_size {
                                corpus.push(CorpusEntry::new(data));
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }

    pub async fn save_corpus(&self) -> Result<()> {
        if let Some(dir) = &self.config.corpus_dir {
            std::fs::create_dir_all(dir)?;
            let corpus = self.corpus.lock().await;
            for (i, entry) in corpus.iter().enumerate() {
                let path = dir.join(format!("corpus_{:06}.bin", i));
                std::fs::write(&path, &entry.data)?;
            }
        }
        Ok(())
    }

    pub async fn save_crashes(&self) -> Result<()> {
        if let Some(dir) = &self.config.crash_dir {
            std::fs::create_dir_all(dir)?;
            let crashes = self.crash_cases.lock().await;
            for (i, crash) in crashes.iter().enumerate() {
                let path = dir.join(format!("crash_{:04}.json", i));
                std::fs::write(&path, serde_json::to_string_pretty(crash)?)?;
            }
        }
        Ok(())
    }

    pub async fn run(&self) -> Result<FuzzStats> {
        self.load_corpus().await?;
        let targets: Vec<_> = self
            .config
            .target_names
            .iter()
            .filter_map(|n| self.registry.get(n))
            .collect();
        if targets.is_empty() {
            anyhow::bail!("no valid targets");
        }
        eprintln!(
            "[fuzz] {} workers, {} targets, seed={}",
            self.config.workers,
            targets.len(),
            self.config.seed
        );

        let mut handles = Vec::new();
        for worker_id in 0..self.config.workers {
            let targets = targets.clone();
            let config = self.config.clone();
            let global_stats = self.global_stats.clone();
            let corpus = self.corpus.clone();
            let crash_cases = self.crash_cases.clone();
            let seed = self.config.seed.wrapping_add(worker_id as u64);
            handles.push(tokio::spawn(async move {
                worker_loop(
                    worker_id,
                    targets,
                    config,
                    global_stats,
                    corpus,
                    crash_cases,
                    seed,
                )
                .await
            }));
        }

        let stats_handle = {
            let global_stats = self.global_stats.clone();
            let config = self.config.clone();
            let corpus = self.corpus.clone();
            let crash_cases = self.crash_cases.clone();
            let harness = self.clone();
            tokio::spawn(async move {
                periodic_tasks(global_stats, config, corpus, crash_cases, harness).await
            })
        };

        for h in handles {
            let _ = h.await;
        }
        stats_handle.abort();
        self.save_corpus().await?;
        self.save_crashes().await?;
        Ok(self.global_stats.snapshot())
    }
}

impl Clone for FuzzHarness {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            registry: TargetRegistry::new(),
            global_stats: self.global_stats.clone(),
            corpus: self.corpus.clone(),
            crash_cases: self.crash_cases.clone(),
            start_time: self.start_time,
        }
    }
}

async fn worker_loop(
    _worker_id: usize,
    targets: Vec<Arc<dyn FuzzTarget>>,
    config: FuzzConfig,
    global_stats: Arc<GlobalStats>,
    corpus: Arc<Mutex<Vec<CorpusEntry>>>,
    _crash_cases: Arc<Mutex<Vec<CrashCase>>>,
    seed: u64,
) -> Result<()> {
    let mut rng = ChaCha20Rng::seed_from_u64(seed);
    let timeout = config.timeout_per_iteration;
    let iteration = 0u64;
    let deadline = config.duration.map(|d| Instant::now() + d);
    loop {
        if let Some(d) = deadline {
            if Instant::now() >= d {
                break;
            }
        }
        if let Some(max_iter) = config.iterations {
            if iteration >= max_iter {
                break;
            }
        }
        let target = targets[rng.gen_range(0..targets.len())].clone();
        let input = if rng.gen_bool(0.3) && {
            let c = corpus.lock().await;
            !c.is_empty()
        } {
            let c = corpus.lock().await;
            let len = c.len();
            let idx = rng.gen_range(0..len);
            let entry = &c[idx];
            let mut u = Unstructured::new(&entry.data);
            target.mutate(&entry.data, &mut u)?
        } else {
            let seed_bytes = rng.gen::<[u8; 32]>();
            let mut u = Unstructured::new(&seed_bytes);
            target.generate_seed(&mut u)?
        };
        if input.len() > config.max_input_size {
            continue;
        }
        let target_name = target.name();
        // Execute is sync in targets; run in blocking pool to avoid blocking tokio
        let input_owned = input.to_vec();
        let _target_name_owned = target_name.to_string();
        let execute_fut = tokio::task::spawn_blocking(move || target.execute(&input_owned));
        let result = tokio::time::timeout(timeout, execute_fut).await;
        global_stats.record_iteration();
        match result {
            Ok(Ok(Ok(()))) => {
                if config.enable_cross_over && rng.gen_bool(0.1) {
                    let mut c = corpus.lock().await;
                    if c.len() < 10000 {
                        c.push(CorpusEntry::new(input));
                        global_stats.record_corpus();
                    }
                }
            }
            Ok(Ok(Err(e))) => {
                global_stats.record_crash(&format!("{}:{}", target_name, e));
            }
            Ok(Err(e)) => {
                global_stats.record_crash(&format!("{}:join error:{}", target_name, e));
            }
            Err(_) => {
                global_stats.record_timeout();
            }
        }
    }
    Ok(())
}

async fn periodic_tasks(
    global_stats: Arc<GlobalStats>,
    config: FuzzConfig,
    corpus: Arc<Mutex<Vec<CorpusEntry>>>,
    _crash_cases: Arc<Mutex<Vec<CrashCase>>>,
    harness: FuzzHarness,
) {
    let mut stats_interval = interval(config.stats_print_interval);
    let mut corpus_interval = interval(config.corpus_save_interval);
    let mut crash_interval = interval(Duration::from_secs(300));
    loop {
        tokio::select! {
            _ = stats_interval.tick() => {
                let stats = global_stats.snapshot();
                let c = corpus.lock().await.len();
                eprintln!("[stats] iter={} crashes={} panics={} timeouts={} corpus={} elapsed={:.1}s", stats.iterations, stats.crashes, stats.panics, stats.timeouts, c, harness.start_time.elapsed().as_secs_f64());
            }
            _ = corpus_interval.tick() => { let _ = harness.save_corpus().await; }
            _ = crash_interval.tick() => { let _ = harness.save_crashes().await; }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn test_harness_creation() {
        let config = FuzzConfig::default();
        let harness = FuzzHarness::new(config);
        assert_eq!(harness.registry.all_names().len(), 5);
    }
}
