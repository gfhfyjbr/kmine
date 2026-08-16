use crate::error::EngineError;
use crate::types::{PrepareMode, ProgressSink};
use serde::de::DeserializeOwned;
use sha1::{Digest, Sha1};
use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tokio::time::MissedTickBehavior;
use tokio_util::sync::CancellationToken;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(300);
const DOWNLOAD_RETRIES: u32 = 2;
const RETRY_BACKOFF: Duration = Duration::from_millis(150);
const DEFAULT_DOWNLOAD_CONCURRENCY: usize = 64;
const MAX_DOWNLOAD_CONCURRENCY: usize = 128;

pub const META_TTL: Duration = Duration::from_secs(3600);

pub fn meta_is_fresh(path: &Path, ttl: Duration) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    let Ok(mtime) = meta.modified() else {
        return false;
    };
    match mtime.elapsed() {
        Ok(age) => age < ttl,
        Err(_) => false,
    }
}

#[derive(Clone)]
pub struct HttpFiles {
    pub client: reqwest::Client,
}

#[derive(Debug, Clone)]
pub struct DownloadJob {
    pub url: String,
    pub dest: PathBuf,
    pub sha1: Option<String>,
    pub size: Option<u64>,
}

pub fn download_concurrency() -> usize {
    std::env::var("KMINE_DOWNLOAD_CONCURRENCY")
        .ok()
        .and_then(|raw| raw.parse().ok())
        .unwrap_or(DEFAULT_DOWNLOAD_CONCURRENCY)
        .clamp(1, MAX_DOWNLOAD_CONCURRENCY)
}

impl HttpFiles {
    pub fn new() -> Result<Self, EngineError> {
        let client = reqwest::Client::builder()
            .user_agent(concat!("kmine/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .http1_only()
            .pool_max_idle_per_host(download_concurrency())
            .tcp_nodelay(true)
            .build()
            .map_err(|err| {
                EngineError::io(
                    PathBuf::from("http-client"),
                    std::io::Error::other(err.to_string()),
                )
            })?;
        Ok(Self { client })
    }

    pub async fn get_json<T: DeserializeOwned>(
        &self,
        url: &str,
        cancel: &CancellationToken,
    ) -> Result<T, EngineError> {
        let response = self.send_ok(url, cancel).await?;
        tokio::select! {
            _ = cancel.cancelled() => Err(EngineError::Cancelled),
            result = response.json::<T>() => result.map_err(|err| reqwest_error(url, err)),
        }
    }

    pub async fn download_sha1(
        &self,
        url: &str,
        dest: &Path,
        expected_sha1: Option<&str>,
        expected_size: Option<u64>,
        cancel: &CancellationToken,
        mode: PrepareMode,
    ) -> Result<(), EngineError> {
        self.download_job(
            &DownloadJob {
                url: url.to_string(),
                dest: dest.to_path_buf(),
                sha1: expected_sha1.map(str::to_string),
                size: expected_size.filter(|size| *size > 0),
            },
            cancel,
            None,
            mode,
        )
        .await
    }

    pub async fn load_meta_bytes(
        &self,
        url: &str,
        dest: &Path,
        mode: PrepareMode,
        cancel: &CancellationToken,
    ) -> Result<Vec<u8>, EngineError> {
        if mode == PrepareMode::Warm && meta_is_fresh(dest, META_TTL) {
            return std::fs::read(dest).map_err(|e| EngineError::io(dest, e));
        }

        match self.force_download_meta(url, dest, cancel, mode).await {
            Ok(()) => std::fs::read(dest).map_err(|e| EngineError::io(dest, e)),
            Err(err) if mode == PrepareMode::Warm => match std::fs::read(dest) {
                Ok(bytes) => Ok(bytes),
                Err(_) => Err(err),
            },
            Err(err) => Err(err),
        }
    }

    pub async fn load_meta_json<T: DeserializeOwned>(
        &self,
        url: &str,
        dest: &Path,
        mode: PrepareMode,
        cancel: &CancellationToken,
    ) -> Result<T, EngineError> {
        if mode == PrepareMode::Warm && meta_is_fresh(dest, META_TTL) {
            return parse_meta_json(dest);
        }

        match self.force_download_meta(url, dest, cancel, mode).await {
            Ok(()) => parse_meta_json(dest),
            Err(download_err) if mode == PrepareMode::Warm => {
                // Prefer parseable stale disk over the download error; corrupt → download error.
                if let Ok(bytes) = std::fs::read(dest) {
                    if let Ok(value) = serde_json::from_slice::<T>(&bytes) {
                        return Ok(value);
                    }
                }
                Err(download_err)
            }
            Err(err) => Err(err),
        }
    }

    /// Download unhashed meta to a sibling path then rename over `dest`.
    /// Avoids `cache_hit` on existing unhashed files and keeps `dest` on failure.
    async fn force_download_meta(
        &self,
        url: &str,
        dest: &Path,
        cancel: &CancellationToken,
        mode: PrepareMode,
    ) -> Result<(), EngineError> {
        let tmp = dest.with_extension("meta-new");
        if tmp.exists() {
            let _ = tokio::fs::remove_file(&tmp).await;
        }
        match self
            .download_sha1(url, &tmp, None, None, cancel, mode)
            .await
        {
            Ok(()) => {
                if let Some(parent) = dest.parent() {
                    if !parent.as_os_str().is_empty() {
                        tokio::fs::create_dir_all(parent)
                            .await
                            .map_err(|e| EngineError::io(parent, e))?;
                    }
                }
                tokio::fs::rename(&tmp, dest)
                    .await
                    .map_err(|e| EngineError::io(dest, e))?;
                Ok(())
            }
            Err(err) => {
                let _ = tokio::fs::remove_file(&tmp).await;
                Err(err)
            }
        }
    }

    pub async fn download_many(
        &self,
        jobs: Vec<DownloadJob>,
        title: &str,
        progress: &dyn ProgressSink,
        cancel: &CancellationToken,
        mode: PrepareMode,
    ) -> Result<(), EngineError> {
        let jobs = schedule_jobs(jobs);
        let file_total = jobs.len() as u64;
        if file_total == 0 {
            progress.set(title, 0, 0);
            return Ok(());
        }

        let jobs = if mode == PrepareMode::Warm {
            let cancel_bg = cancel.clone();
            let (all_hit, jobs) = spawn_blocking_io(Path::new("download"), move || {
                let mut all_hit = true;
                for job in &jobs {
                    if cancel_bg.is_cancelled() {
                        return Err(EngineError::Cancelled);
                    }
                    if !cache_hit(&job.dest, job.sha1.as_deref(), job.size, PrepareMode::Warm)? {
                        all_hit = false;
                        break;
                    }
                }
                Ok((all_hit, jobs))
            })
            .await?;
            if all_hit {
                progress.set(title, file_total, file_total);
                return Ok(());
            }
            jobs
        } else {
            jobs
        };

        let total_bytes = jobs
            .iter()
            .map(|job| job.size)
            .try_fold(0u64, |acc, size| Some(acc.saturating_add(size?)));
        let byte_mode = total_bytes.unwrap_or(0) > 0;
        let total_bytes = total_bytes.unwrap_or(0);
        report_batch(progress, title, 0, file_total, 0, total_bytes, byte_mode);

        let meter = ByteMeter::default();
        let files_done = Arc::new(AtomicU64::new(0));
        let sem = Arc::new(Semaphore::new(download_concurrency()));
        let mut set = JoinSet::new();
        for job in jobs {
            if cancel.is_cancelled() {
                set.abort_all();
                return Err(EngineError::Cancelled);
            }
            let http = self.clone();
            let cancel = cancel.clone();
            let sem = sem.clone();
            let meter = meter.clone();
            set.spawn(async move {
                let _permit = tokio::select! {
                    _ = cancel.cancelled() => return Err(EngineError::Cancelled),
                    result = sem.acquire_owned() => result.map_err(|err| {
                        EngineError::io(job.dest.clone(), std::io::Error::other(err.to_string()))
                    })?,
                };
                http.download_job(&job, &cancel, Some(meter), mode).await
            });
        }

        let mut ticker = tokio::time::interval(Duration::from_millis(100));
        ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    set.abort_all();
                    return Err(EngineError::Cancelled);
                }
                _ = ticker.tick(), if byte_mode => {
                    report_batch(
                        progress,
                        title,
                        files_done.load(Ordering::Relaxed),
                        file_total,
                        meter.get(),
                        total_bytes,
                        true,
                    );
                }
                joined = set.join_next() => {
                    match joined {
                        None => break,
                        Some(Ok(Ok(()))) => {
                            let done = files_done.fetch_add(1, Ordering::Relaxed) + 1;
                            report_batch(
                                progress,
                                title,
                                done,
                                file_total,
                                meter.get(),
                                total_bytes,
                                byte_mode,
                            );
                        }
                        Some(Ok(Err(err))) => {
                            set.abort_all();
                            return Err(err);
                        }
                        Some(Err(join_err)) if join_err.is_cancelled() => {
                            set.abort_all();
                            return Err(EngineError::Cancelled);
                        }
                        Some(Err(join_err)) => {
                            set.abort_all();
                            return Err(EngineError::io(
                                PathBuf::from("download"),
                                std::io::Error::other(join_err.to_string()),
                            ));
                        }
                    }
                }
            }
        }
        report_batch(
            progress,
            title,
            file_total,
            file_total,
            total_bytes,
            total_bytes,
            byte_mode,
        );
        Ok(())
    }

    async fn download_job(
        &self,
        job: &DownloadJob,
        cancel: &CancellationToken,
        meter: Option<ByteMeter>,
        mode: PrepareMode,
    ) -> Result<(), EngineError> {
        if cancel.is_cancelled() {
            return Err(EngineError::Cancelled);
        }
        let dest_buf = job.dest.clone();
        let expected = job.sha1.clone();
        let expected_size = job.size;
        let hit = spawn_blocking_io(&job.dest, move || {
            cache_hit(&dest_buf, expected.as_deref(), expected_size, mode)
        })
        .await?;
        if hit {
            if let Some(meter) = meter {
                meter.add(job.size.unwrap_or(0));
            }
            return Ok(());
        }

        let dest_buf = job.dest.clone();
        spawn_blocking_io(&job.dest, move || prepare_dest(&dest_buf)).await?;

        let part = job.dest.with_extension("part");
        let mut last_err = None;
        for attempt in 0..=DOWNLOAD_RETRIES {
            if cancel.is_cancelled() {
                let _ = tokio::fs::remove_file(&part).await;
                return Err(EngineError::Cancelled);
            }
            match self
                .download_to_part(
                    &job.url,
                    &job.dest,
                    &part,
                    job.sha1.as_deref(),
                    cancel,
                    meter.as_ref(),
                )
                .await
            {
                Ok(()) => return Ok(()),
                Err(err) => {
                    let _ = tokio::fs::remove_file(&part).await;
                    if !is_retryable(&err) || attempt == DOWNLOAD_RETRIES {
                        return Err(err);
                    }
                    last_err = Some(err);
                    tokio::select! {
                        _ = cancel.cancelled() => return Err(EngineError::Cancelled),
                        _ = tokio::time::sleep(RETRY_BACKOFF * (attempt + 1)) => {}
                    }
                }
            }
        }
        Err(last_err.unwrap_or(EngineError::Cancelled))
    }

    async fn send_ok(
        &self,
        url: &str,
        cancel: &CancellationToken,
    ) -> Result<reqwest::Response, EngineError> {
        tokio::select! {
            _ = cancel.cancelled() => Err(EngineError::Cancelled),
            result = self.client.get(url).send() => {
                let response = result.map_err(|err| reqwest_error(url, err))?;
                let status = response.status();
                if status.is_success() {
                    Ok(response)
                } else {
                    Err(EngineError::Http {
                        url: url.to_string(),
                        status: status.as_u16(),
                    })
                }
            }
        }
    }

    async fn download_to_part(
        &self,
        url: &str,
        dest: &Path,
        part: &Path,
        expected_sha1: Option<&str>,
        cancel: &CancellationToken,
        meter: Option<&ByteMeter>,
    ) -> Result<(), EngineError> {
        let mut response = self.send_ok(url, cancel).await?;
        let mut file = tokio::fs::File::create(part)
            .await
            .map_err(|e| EngineError::io(part, e))?;
        let mut hasher = Sha1::new();
        let mut written = 0u64;
        let result = async {
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => return Err(EngineError::Cancelled),
                    chunk = response.chunk() => {
                        match chunk {
                            Ok(Some(bytes)) => {
                                hasher.update(&bytes);
                                file.write_all(&bytes)
                                    .await
                                    .map_err(|e| EngineError::io(part, e))?;
                                let n = bytes.len() as u64;
                                written += n;
                                if let Some(meter) = meter {
                                    meter.add(n);
                                }
                            }
                            Ok(None) => break,
                            Err(err) => return Err(reqwest_error(url, err)),
                        }
                    }
                }
            }
            file.flush().await.map_err(|e| EngineError::io(part, e))?;
            drop(file);

            let actual = hex::encode(hasher.finalize());
            if let Some(expected) = expected_sha1 {
                let expected = expected.to_ascii_lowercase();
                if actual != expected {
                    return Err(EngineError::ChecksumMismatch {
                        path: dest.to_path_buf(),
                        expected,
                        actual,
                    });
                }
            }
            tokio::fs::rename(part, dest)
                .await
                .map_err(|e| EngineError::io(dest, e))?;
            Ok(())
        }
        .await;
        if result.is_err() {
            if let Some(meter) = meter {
                meter.sub(written);
            }
        }
        result
    }
}

fn parse_meta_json<T: DeserializeOwned>(dest: &Path) -> Result<T, EngineError> {
    let bytes = std::fs::read(dest).map_err(|e| EngineError::io(dest, e))?;
    serde_json::from_slice(&bytes)
        .map_err(|e| EngineError::io(dest, std::io::Error::other(e.to_string())))
}

fn cache_hit(
    dest: &Path,
    expected_sha1: Option<&str>,
    expected_size: Option<u64>,
    mode: PrepareMode,
) -> Result<bool, EngineError> {
    let meta = match std::fs::metadata(dest) {
        Ok(meta) => meta,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(err) => return Err(EngineError::io(dest, err)),
    };
    if !meta.is_file() || meta.len() == 0 {
        return Ok(false);
    }
    if expected_size.is_some_and(|size| meta.len() != size) {
        return Ok(false);
    }
    let must_hash = match (mode, expected_sha1, expected_size) {
        (PrepareMode::Warm, _, Some(_)) => false,
        (PrepareMode::Warm, Some(_), None) => true,
        (PrepareMode::Warm, None, None) => false,
        (PrepareMode::Verify, Some(_), _) => true,
        (PrepareMode::Verify, None, _) => false,
    };
    if !must_hash {
        return Ok(true);
    }
    let expected = expected_sha1.expect("must_hash implies sha1");
    Ok(hash_file(dest)? == expected.to_ascii_lowercase())
}

fn hash_file(path: &Path) -> Result<String, EngineError> {
    let mut file = std::fs::File::open(path).map_err(|e| EngineError::io(path, e))?;
    let mut hasher = Sha1::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf).map_err(|e| EngineError::io(path, e))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn prepare_dest(dest: &Path) -> Result<(), EngineError> {
    if dest.exists() {
        std::fs::remove_file(dest).map_err(|e| EngineError::io(dest, e))?;
    }
    if let Some(parent) = dest.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|e| EngineError::io(parent, e))?;
        }
    }
    Ok(())
}

fn schedule_jobs(jobs: Vec<DownloadJob>) -> Vec<DownloadJob> {
    let mut jobs = dedupe_jobs(jobs);
    jobs.sort_by(|a, b| b.size.unwrap_or(0).cmp(&a.size.unwrap_or(0)));
    jobs
}

fn dedupe_jobs(jobs: Vec<DownloadJob>) -> Vec<DownloadJob> {
    let mut seen = HashSet::new();
    jobs.into_iter()
        .filter(|job| seen.insert(job.dest.clone()))
        .collect()
}

fn report_batch(
    progress: &dyn ProgressSink,
    title: &str,
    files_done: u64,
    file_total: u64,
    bytes_done: u64,
    bytes_total: u64,
    byte_mode: bool,
) {
    if byte_mode {
        progress.set(
            &format!("{title} · {files_done}/{file_total}"),
            bytes_done.min(bytes_total),
            bytes_total,
        );
    } else {
        progress.set(title, files_done, file_total);
    }
}

#[derive(Clone, Default)]
struct ByteMeter {
    done: Arc<AtomicU64>,
}

impl ByteMeter {
    fn add(&self, n: u64) {
        if n > 0 {
            self.done.fetch_add(n, Ordering::Relaxed);
        }
    }

    fn sub(&self, n: u64) {
        if n > 0 {
            self.done.fetch_sub(n, Ordering::Relaxed);
        }
    }

    fn get(&self) -> u64 {
        self.done.load(Ordering::Relaxed)
    }
}

fn is_retryable(err: &EngineError) -> bool {
    match err {
        EngineError::Cancelled => false,
        EngineError::Http { status, .. } => matches!(*status, 408 | 425 | 429 | 500..=599),
        EngineError::ChecksumMismatch { .. } | EngineError::Io { .. } => true,
        _ => false,
    }
}

async fn spawn_blocking_io<T: Send + 'static>(
    path: &Path,
    f: impl FnOnce() -> Result<T, EngineError> + Send + 'static,
) -> Result<T, EngineError> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|err| EngineError::io(path, std::io::Error::other(err.to_string())))?
}

fn reqwest_error(url: &str, err: reqwest::Error) -> EngineError {
    if let Some(status) = err.status() {
        EngineError::Http {
            url: url.to_string(),
            status: status.as_u16(),
        }
    } else {
        EngineError::io(PathBuf::from(url), std::io::Error::other(err.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::{DownloadJob, HttpFiles, cache_hit, meta_is_fresh, schedule_jobs};
    use crate::error::EngineError;
    use crate::types::{PrepareMode, ProgressSink};
    use sha1::{Digest, Sha1};
    use std::path::Path;
    use std::sync::Mutex;
    use std::time::Duration;
    use tokio_util::sync::CancellationToken;

    struct NoopProgress;
    impl ProgressSink for NoopProgress {
        fn set(&self, _title: &str, _done: u64, _total: u64) {}
    }

    struct RecordingProgress {
        events: Mutex<Vec<(String, u64, u64)>>,
    }

    impl RecordingProgress {
        fn new() -> Self {
            Self {
                events: Mutex::new(Vec::new()),
            }
        }
    }

    impl ProgressSink for RecordingProgress {
        fn set(&self, title: &str, done: u64, total: u64) {
            self.events
                .lock()
                .unwrap()
                .push((title.to_string(), done, total));
        }
    }

    fn sha1_hex(bytes: &[u8]) -> String {
        hex::encode(Sha1::digest(bytes))
    }

    #[test]
    fn warm_size_match_does_not_need_sha1() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("a.bin");
        std::fs::write(&dest, b"wrong-bytes-same-len!").unwrap();
        // 21 bytes. SHA-1 of "abc" would miss. Warm + matching size must still hit.
        assert!(cache_hit(&dest, Some("deadbeef"), Some(21), PrepareMode::Warm).unwrap());
    }

    #[test]
    fn verify_size_match_still_checks_sha1() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("a.bin");
        std::fs::write(&dest, b"wrong-bytes-same-len!").unwrap();
        assert!(!cache_hit(&dest, Some("deadbeef"), Some(21), PrepareMode::Verify).unwrap());
    }

    #[test]
    fn warm_unknown_size_hashes() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("a.bin");
        let body = b"abc";
        std::fs::write(&dest, body).unwrap();
        let hash = sha1_hex(body);
        assert!(cache_hit(&dest, Some(&hash), None, PrepareMode::Warm).unwrap());
        assert!(!cache_hit(&dest, Some("deadbeef"), None, PrepareMode::Warm).unwrap());
    }

    #[tokio::test]
    async fn warm_size_hit_makes_zero_http() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .respond_with(wiremock::ResponseTemplate::new(500))
            .expect(0)
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("f.bin");
        std::fs::write(&dest, b"abc").unwrap();
        let jobs = vec![DownloadJob {
            url: format!("{}/f", server.uri()),
            dest,
            sha1: Some("ffffffff".into()),
            size: Some(3),
        }];
        HttpFiles::new()
            .unwrap()
            .download_many(
                jobs,
                "Files",
                &NoopProgress,
                &CancellationToken::new(),
                PrepareMode::Warm,
            )
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn warm_download_sha1_size_hit_makes_zero_http() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .respond_with(wiremock::ResponseTemplate::new(500))
            .expect(0)
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("f.bin");
        std::fs::write(&dest, b"abc").unwrap();
        HttpFiles::new()
            .unwrap()
            .download_sha1(
                &format!("{}/f", server.uri()),
                &dest,
                Some("ffffffff"),
                Some(3),
                &CancellationToken::new(),
                PrepareMode::Warm,
            )
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn warm_all_hit_batch_reports_once() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .respond_with(wiremock::ResponseTemplate::new(500))
            .expect(0)
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let jobs: Vec<DownloadJob> = (0..3)
            .map(|i| {
                let dest = dir.path().join(format!("f{i}.bin"));
                std::fs::write(&dest, b"abc").unwrap();
                DownloadJob {
                    url: format!("{}/f{i}", server.uri()),
                    dest,
                    sha1: Some("ffffffff".into()),
                    size: Some(3),
                }
            })
            .collect();
        let progress = RecordingProgress::new();
        HttpFiles::new()
            .unwrap()
            .download_many(
                jobs,
                "Files",
                &progress,
                &CancellationToken::new(),
                PrepareMode::Warm,
            )
            .await
            .unwrap();
        let events = progress.events.lock().unwrap().clone();
        assert_eq!(events, vec![("Files".into(), 3, 3)]);
    }

    #[tokio::test]
    async fn downloads_and_verifies_sha1() {
        let server = wiremock::MockServer::start().await;
        let body = b"abc";
        let hash = sha1_hex(body);
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .respond_with(
                wiremock::ResponseTemplate::new(200).set_body_raw(body.as_slice(), "text/plain"),
            )
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("f.bin");
        let http = HttpFiles::new().unwrap();
        http.download_sha1(
            &format!("{}/f", server.uri()),
            &dest,
            Some(&hash),
            None,
            &CancellationToken::new(),
            PrepareMode::Verify,
        )
        .await
        .unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), body);
    }

    #[tokio::test]
    async fn cache_hit_skips_network() {
        let server = wiremock::MockServer::start().await;
        let body = b"abc";
        let hash = sha1_hex(body);
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .respond_with(wiremock::ResponseTemplate::new(500))
            .expect(0)
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("f.bin");
        std::fs::write(&dest, body).unwrap();
        let http = HttpFiles::new().unwrap();
        http.download_sha1(
            &format!("{}/f", server.uri()),
            &dest,
            Some(&hash),
            None,
            &CancellationToken::new(),
            PrepareMode::Verify,
        )
        .await
        .unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), body);
    }

    #[tokio::test]
    async fn bad_sha1_errors() {
        let server = wiremock::MockServer::start().await;
        let body = b"abc";
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .respond_with(
                wiremock::ResponseTemplate::new(200).set_body_raw(body.as_slice(), "text/plain"),
            )
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("f.bin");
        let http = HttpFiles::new().unwrap();
        let err = http
            .download_sha1(
                &format!("{}/f", server.uri()),
                &dest,
                Some("deadbeef"),
                None,
                &CancellationToken::new(),
                PrepareMode::Verify,
            )
            .await
            .unwrap_err();
        assert!(matches!(err, EngineError::ChecksumMismatch { .. }));
    }

    #[tokio::test]
    async fn cancel_stops_download() {
        let cancel = CancellationToken::new();
        cancel.cancel();
        let err = HttpFiles::new()
            .unwrap()
            .download_sha1(
                "http://127.0.0.1:1/",
                Path::new("/tmp/x"),
                None,
                None,
                &cancel,
                PrepareMode::Warm,
            )
            .await
            .unwrap_err();
        assert!(matches!(err, EngineError::Cancelled));
    }

    #[tokio::test]
    async fn download_many_writes_all_files() {
        let server = wiremock::MockServer::start().await;
        let n = 8usize;
        for i in 0..n {
            let body = format!("body-{i}");
            wiremock::Mock::given(wiremock::matchers::method("GET"))
                .and(wiremock::matchers::path(format!("/f{i}")))
                .respond_with(
                    wiremock::ResponseTemplate::new(200)
                        .set_body_raw(body.into_bytes(), "text/plain"),
                )
                .mount(&server)
                .await;
        }
        let dir = tempfile::tempdir().unwrap();
        let jobs: Vec<DownloadJob> = (0..n)
            .map(|i| DownloadJob {
                url: format!("{}/f{i}", server.uri()),
                dest: dir.path().join(format!("f{i}.bin")),
                sha1: None,
                size: None,
            })
            .collect();
        HttpFiles::new()
            .unwrap()
            .download_many(
                jobs,
                "Files",
                &NoopProgress,
                &CancellationToken::new(),
                PrepareMode::Warm,
            )
            .await
            .unwrap();
        for i in 0..n {
            let got = std::fs::read_to_string(dir.path().join(format!("f{i}.bin"))).unwrap();
            assert_eq!(got, format!("body-{i}"));
        }
    }

    #[tokio::test]
    async fn download_many_stops_on_cancel() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .respond_with(
                wiremock::ResponseTemplate::new(200)
                    .set_delay(std::time::Duration::from_secs(5))
                    .set_body_raw(b"slow".as_slice(), "text/plain"),
            )
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let jobs = (0..4)
            .map(|i| DownloadJob {
                url: format!("{}/slow{i}", server.uri()),
                dest: dir.path().join(format!("s{i}.bin")),
                sha1: None,
                size: None,
            })
            .collect();
        let cancel = CancellationToken::new();
        let cancel_bg = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
            cancel_bg.cancel();
        });
        let err = HttpFiles::new()
            .unwrap()
            .download_many(jobs, "Files", &NoopProgress, &cancel, PrepareMode::Warm)
            .await
            .unwrap_err();
        assert!(matches!(err, EngineError::Cancelled));
    }

    #[test]
    fn schedule_jobs_largest_first() {
        let job = |name: &str, size: u64| DownloadJob {
            url: name.into(),
            dest: std::path::PathBuf::from(name),
            sha1: None,
            size: Some(size),
        };
        let jobs = schedule_jobs(vec![job("a", 1), job("b", 50), job("c", 10)]);
        assert_eq!(jobs[0].dest.as_os_str(), "b");
        assert_eq!(jobs[1].dest.as_os_str(), "c");
        assert_eq!(jobs[2].dest.as_os_str(), "a");
    }

    #[tokio::test]
    async fn retries_transient_http_error() {
        let server = wiremock::MockServer::start().await;
        let body = b"ok";
        let hash = sha1_hex(body);
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .respond_with(wiremock::ResponseTemplate::new(503))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .respond_with(
                wiremock::ResponseTemplate::new(200).set_body_raw(body.as_slice(), "text/plain"),
            )
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("retry.bin");
        HttpFiles::new()
            .unwrap()
            .download_sha1(
                &format!("{}/r", server.uri()),
                &dest,
                Some(&hash),
                None,
                &CancellationToken::new(),
                PrepareMode::Verify,
            )
            .await
            .unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), body);
    }

    #[test]
    fn meta_is_fresh_respects_ttl() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("m.json");
        std::fs::write(&path, b"{}").unwrap();
        assert!(meta_is_fresh(&path, Duration::from_secs(3600)));
        let file = std::fs::File::options().write(true).open(&path).unwrap();
        file.set_modified(std::time::SystemTime::now() - Duration::from_secs(3601))
            .unwrap();
        assert!(!meta_is_fresh(&path, Duration::from_secs(3600)));
    }

    #[tokio::test]
    async fn load_meta_json_warm_fresh_zero_http() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .respond_with(wiremock::ResponseTemplate::new(500))
            .expect(0)
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("m.json");
        std::fs::write(&dest, br#"{"ok":true}"#).unwrap();
        let v: serde_json::Value = HttpFiles::new()
            .unwrap()
            .load_meta_json(
                &format!("{}/m", server.uri()),
                &dest,
                PrepareMode::Warm,
                &CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(v["ok"], true);
    }

    #[tokio::test]
    async fn load_meta_json_verify_always_fetches() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .respond_with(
                wiremock::ResponseTemplate::new(200)
                    .set_body_raw(br#"{"ok":false}"#, "application/json"),
            )
            .expect(1)
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("m.json");
        std::fs::write(&dest, br#"{"ok":true}"#).unwrap();
        let v: serde_json::Value = HttpFiles::new()
            .unwrap()
            .load_meta_json(
                &format!("{}/m", server.uri()),
                &dest,
                PrepareMode::Verify,
                &CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(v["ok"], false);
    }

    #[tokio::test]
    async fn load_meta_json_warm_stale_falls_back_when_download_fails() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .respond_with(wiremock::ResponseTemplate::new(500))
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("m.json");
        std::fs::write(&dest, br#"{"ok":true}"#).unwrap();
        let file = std::fs::File::options().write(true).open(&dest).unwrap();
        file.set_modified(std::time::SystemTime::now() - Duration::from_secs(4000))
            .unwrap();
        drop(file);
        let v: serde_json::Value = HttpFiles::new()
            .unwrap()
            .load_meta_json(
                &format!("{}/m", server.uri()),
                &dest,
                PrepareMode::Warm,
                &CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(v["ok"], true);
    }

    #[tokio::test]
    async fn load_meta_json_warm_stale_corrupt_returns_download_error() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .respond_with(wiremock::ResponseTemplate::new(500))
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("m.json");
        std::fs::write(&dest, b"{").unwrap();
        let file = std::fs::File::options().write(true).open(&dest).unwrap();
        file.set_modified(std::time::SystemTime::now() - Duration::from_secs(4000))
            .unwrap();
        drop(file);
        let err = HttpFiles::new()
            .unwrap()
            .load_meta_json::<serde_json::Value>(
                &format!("{}/m", server.uri()),
                &dest,
                PrepareMode::Warm,
                &CancellationToken::new(),
            )
            .await
            .unwrap_err();
        assert!(
            matches!(err, EngineError::Http { status: 500, .. }),
            "expected download Http error, got {err:?}"
        );
    }
}
