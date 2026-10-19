use std::sync::{atomic::{AtomicBool, AtomicU64, Ordering}, LazyLock, Mutex};
use std::time::{Duration, Instant};

struct Bucket { tokens: f64, updated: Instant }
struct Limiter { rate: AtomicU64, bucket: Mutex<Bucket> }
const MAX_RATE: u64 = 10 * 1024 * 1024 * 1024;
static LIMITER: LazyLock<Limiter> = LazyLock::new(|| Limiter::new(load()));
static SAVE_LOCK: Mutex<()> = Mutex::new(());
fn load() -> u64 {
    std::fs::read(crate::storage::settings().join("downloads.json")).ok()
        .and_then(|bytes| serde_json::from_slice::<u64>(&bytes).ok())
        .filter(|rate| *rate <= MAX_RATE).unwrap_or(0)
}
impl Limiter {
    fn new(rate: u64) -> Self { Self {rate: AtomicU64::new(rate), bucket: Mutex::new(Bucket {tokens: 0., updated: Instant::now()})} }
    fn set(&self, rate: u64) {
        let mut bucket = self.bucket.lock().unwrap_or_else(|e| e.into_inner());
        self.rate.store(rate, Ordering::SeqCst);
        bucket.tokens = 0.; bucket.updated = Instant::now();
    }
    async fn consume(&self, bytes: usize, cancel: &AtomicBool) -> Result<(), String> {
        self.consume_controlled(bytes,cancel,None).await
    }
    async fn consume_controlled(&self, bytes:usize,cancel:&AtomicBool,paused:Option<&AtomicBool>)->Result<(),String>{
        let mut remaining = bytes as f64;
        while remaining > 0. {
            if cancel.load(Ordering::SeqCst) {return Err("Загрузка остановлена. Готовые файлы сохранены для повторной попытки".into());}
            if paused.is_some_and(|p|p.load(Ordering::SeqCst)){tokio::time::sleep(Duration::from_millis(100)).await;continue;}
            let delay = {
                let mut bucket = self.bucket.lock().unwrap_or_else(|e| e.into_inner());
                let rate = self.rate.load(Ordering::SeqCst);
                if rate == 0 {return Ok(());}
                let now = Instant::now();
                bucket.tokens = (bucket.tokens + now.duration_since(bucket.updated).as_secs_f64() * rate as f64).min((rate as f64 / 10.).max(1.));
                bucket.updated = now;
                let granted = remaining.min(bucket.tokens.floor());
                bucket.tokens -= granted; remaining -= granted;
                Duration::from_secs_f64((remaining / rate as f64).clamp(0.001, 0.1))
            };
            if remaining > 0. {tokio::time::sleep(delay).await;}
        }
        Ok(())
    }
}
pub async fn throttle(bytes: usize, cancel: &AtomicBool) -> Result<(), String> { LIMITER.consume(bytes, cancel).await }
pub async fn throttle_paused(bytes:usize,cancel:&AtomicBool,paused:&AtomicBool)->Result<(),String>{LIMITER.consume_controlled(bytes,cancel,Some(paused)).await}
#[tauri::command]
pub fn download_limit_get() -> u64 { LIMITER.rate.load(Ordering::SeqCst) }
#[tauri::command]
pub fn download_limit_set(bytes_per_second: u64) -> Result<u64, String> {
    if bytes_per_second > MAX_RATE {return Err("Invalid download limit".into());}
    let _guard = SAVE_LOCK.lock().map_err(|e| e.to_string())?;
    crate::engine::install::atomic(&crate::storage::settings().join("downloads.json"), bytes_per_second.to_string().as_bytes())?;
    LIMITER.set(bytes_per_second);
    Ok(bytes_per_second)
}
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn shared_limit_and_live_changes() {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_time().build().unwrap();
        runtime.block_on(async {
            let limiter = std::sync::Arc::new(Limiter::new(1000));
            let start=Instant::now();
            let mut tasks=tokio::task::JoinSet::new();
            for _ in 0..4 {let limiter=limiter.clone();tasks.spawn(async move {limiter.consume(100,&AtomicBool::new(false)).await.unwrap();});}
            while let Some(result)=tasks.join_next().await {result.unwrap();}
            assert!(start.elapsed() >= Duration::from_millis(350));
            let copy=limiter.clone();
            let task=tokio::spawn(async move {copy.consume(100000,&AtomicBool::new(false)).await});
            tokio::time::sleep(Duration::from_millis(20)).await;
            limiter.set(0);
            tokio::time::timeout(Duration::from_secs(1),task).await.unwrap().unwrap().unwrap();
            limiter.set(1);
            assert!(limiter.consume(1000,&AtomicBool::new(true)).await.is_err());
        });
    }
    #[test] fn rejects_invalid_limit_before_writing() {assert!(download_limit_set(MAX_RATE+1).is_err());}
}
