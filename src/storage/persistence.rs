//! Persistence layer with AOF (Append-Only File) support.
//!
//! The on-disk format is a stream of RESP-encoded command frames — exactly
//! the bytes a client sent over the wire. Recovery feeds the file through
//! the same `RespCodec` the network layer uses and replays each command
//! through the regular `CommandRegistry` dispatch path. This guarantees
//! that what's persisted matches what was executed and removes any need
//! for a parallel command enum or text-based serialization.

use crate::commands::ResponseValue;
use crate::config::{Config, FsyncPolicy};
use crate::error::{Result, RustyPotatoError};
use crate::network::protocol::RespCodec;
use crate::storage::memory::{StoredValue, ValueType};
use bytes::Bytes;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{mpsc, oneshot};
use tokio::time::interval;
use tracing::{debug, error, info};

/// Default size for the channel between command-dispatch and the AOF writer.
/// Acts as a backpressure boundary — when full, callers wait.
const AOF_CHANNEL_CAPACITY: usize = 8192;

/// Render one entry from a `MemoryStore::snapshot()` into the
/// RESP-encoded command frames needed to recreate it on replay:
///
/// - `String`/`Integer` → `SET key value`
/// - `Hash`             → `HSET key f1 v1 f2 v2 ...` (variadic)
/// - `List`             → skipped (no list commands yet)
///
/// If the entry has a TTL, a follow-up `PEXPIREAT key <unix-ms>` is
/// emitted so the deadline survives the rewrite — relative `EX`/`PX`
/// would drift across the rewrite window.
fn encode_snapshot_entry(key: &str, value: &StoredValue, frames: &mut Vec<Bytes>) -> Result<()> {
    let mut codec = RespCodec::new();

    let array = match &value.value {
        ValueType::String(b) => Some(ResponseValue::Array(vec![
            ResponseValue::bulk("SET"),
            ResponseValue::bulk(key.to_string()),
            ResponseValue::bulk(b.clone()),
        ])),
        ValueType::Integer(i) => Some(ResponseValue::Array(vec![
            ResponseValue::bulk("SET"),
            ResponseValue::bulk(key.to_string()),
            ResponseValue::bulk(i.to_string()),
        ])),
        ValueType::Hash(h) => {
            if h.is_empty() {
                None
            } else {
                let mut parts = Vec::with_capacity(2 + h.len() * 2);
                parts.push(ResponseValue::bulk("HSET"));
                parts.push(ResponseValue::bulk(key.to_string()));
                for (f, v) in h {
                    parts.push(ResponseValue::bulk(f.clone()));
                    parts.push(ResponseValue::bulk(v.clone()));
                }
                Some(ResponseValue::Array(parts))
            }
        }
        // Lists not yet supported in the rewrite (no LPUSH/RPUSH command
        // exists). When list commands land, add a branch here.
        ValueType::List(_) => None,
    };

    if let Some(array) = array {
        let frame = codec
            .encode(&array)
            .map_err(|e| RustyPotatoError::PersistenceError {
                message: format!("Failed to encode snapshot entry for {key}: {e}"),
                source: None,
                recoverable: false,
            })?;
        frames.push(Bytes::from(frame));
    }

    // TTL preservation via PEXPIREAT (absolute, in ms since epoch).
    // The `expires_at` is monotonic, so we translate it into wall
    // clock by computing the remaining duration from now.
    if let Some(expires_at) = value.expires_at {
        let now = Instant::now();
        if expires_at > now {
            let remaining = expires_at - now;
            let now_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| RustyPotatoError::PersistenceError {
                    message: "system clock before UNIX epoch".to_string(),
                    source: None,
                    recoverable: false,
                })?
                .as_millis() as u64;
            let target_ms = now_ms.saturating_add(remaining.as_millis() as u64);
            let pexpireat = ResponseValue::Array(vec![
                ResponseValue::bulk("PEXPIREAT"),
                ResponseValue::bulk(key.to_string()),
                ResponseValue::bulk(target_ms.to_string()),
            ]);
            let frame =
                codec
                    .encode(&pexpireat)
                    .map_err(|e| RustyPotatoError::PersistenceError {
                        message: format!("Failed to encode PEXPIREAT for {key}: {e}"),
                        source: None,
                        recoverable: false,
                    })?;
            frames.push(Bytes::from(frame));
        }
    }

    Ok(())
}

/// Render a full snapshot (from `MemoryStore::snapshot()`) into RESP
/// frames suitable for `PersistenceManager::rewrite_aof`.
pub fn snapshot_to_frames(snapshot: &[(String, StoredValue)]) -> Result<Vec<Bytes>> {
    let mut frames = Vec::with_capacity(snapshot.len());
    for (key, value) in snapshot {
        encode_snapshot_entry(key, value, &mut frames)?;
    }
    Ok(frames)
}

/// Messages the writer task accepts from the rest of the system.
#[derive(Debug)]
enum WriterCmd {
    /// Append a RESP-encoded command frame to the AOF.
    Record(Bytes),
    /// Force a flush+fsync now; reply when complete.
    Flush(oneshot::Sender<Result<()>>),
    /// Report current file size; reply with the answer.
    FileSize(oneshot::Sender<Result<u64>>),
    /// Atomically replace the live AOF file with `temp_path`. The
    /// writer task flushes its current buffer (which gets discarded
    /// with the old file), runs the rename, then reopens the new
    /// file in append mode for subsequent writes.
    ///
    /// Caller must hold a write barrier preventing new mutations
    /// during this operation — otherwise mutations between the
    /// snapshot and the swap are lost.
    SwapFile {
        temp_path: PathBuf,
        reply: oneshot::Sender<Result<()>>,
    },
    /// Stop the writer task gracefully (final flush, then exit).
    Shutdown(oneshot::Sender<Result<()>>),
}

/// Owns the AOF file handle and runs entirely inside the writer task.
/// Never shared across tasks.
struct AofWriter {
    file: File,
    buffer: Vec<u8>,
    fsync_policy: FsyncPolicy,
    last_flush: Instant,
    flush_interval: Duration,
    file_path: PathBuf,
}

impl AofWriter {
    async fn open(config: &Config) -> Result<Self> {
        let file_path = config.storage.aof_path.clone();

        if let Some(parent) = file_path.parent() {
            if !parent.as_os_str().is_empty() {
                tokio::fs::create_dir_all(parent).await.map_err(|e| {
                    RustyPotatoError::PersistenceError {
                        message: format!(
                            "Failed to create AOF directory {}: {e}",
                            parent.display()
                        ),
                        source: Some(e),
                        recoverable: false,
                    }
                })?;
            }
        }

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file_path)
            .await
            .map_err(|e| RustyPotatoError::PersistenceError {
                message: format!("Failed to open AOF {}: {e}", file_path.display()),
                source: Some(e),
                recoverable: false,
            })?;

        info!("AOF file opened: {}", file_path.display());

        Ok(Self {
            file,
            buffer: Vec::with_capacity(64 * 1024),
            fsync_policy: config.storage.aof_fsync_policy.clone(),
            last_flush: Instant::now(),
            flush_interval: Duration::from_secs(1),
            file_path,
        })
    }

    /// Append a frame to the in-memory buffer; flush per fsync policy.
    async fn record(&mut self, bytes: Bytes) -> Result<()> {
        self.buffer.extend_from_slice(&bytes);

        match self.fsync_policy {
            FsyncPolicy::Always => self.flush_to_disk(true).await,
            FsyncPolicy::EverySecond => {
                if self.last_flush.elapsed() >= self.flush_interval {
                    self.flush_to_disk(true).await
                } else {
                    Ok(())
                }
            }
            FsyncPolicy::Never => {
                if self.buffer.len() >= 1024 * 1024 {
                    self.flush_to_disk(false).await
                } else {
                    Ok(())
                }
            }
        }
    }

    /// Periodic tick — used by EverySecond policy when no writes are happening.
    async fn periodic_flush(&mut self) -> Result<()> {
        match self.fsync_policy {
            FsyncPolicy::EverySecond if self.last_flush.elapsed() >= self.flush_interval => {
                self.flush_to_disk(true).await
            }
            _ => Ok(()),
        }
    }

    /// Force flush + optional fsync.
    async fn flush_to_disk(&mut self, sync: bool) -> Result<()> {
        if !self.buffer.is_empty() {
            self.file.write_all(&self.buffer).await.map_err(|e| {
                RustyPotatoError::PersistenceError {
                    message: format!("AOF write failed: {e}"),
                    source: Some(e),
                    recoverable: true,
                }
            })?;
            self.buffer.clear();
        }

        self.file
            .flush()
            .await
            .map_err(|e| RustyPotatoError::PersistenceError {
                message: format!("AOF flush failed: {e}"),
                source: Some(e),
                recoverable: true,
            })?;

        if sync {
            self.file
                .sync_all()
                .await
                .map_err(|e| RustyPotatoError::PersistenceError {
                    message: format!("AOF fsync failed: {e}"),
                    source: Some(e),
                    recoverable: true,
                })?;
        }

        self.last_flush = Instant::now();
        Ok(())
    }

    async fn current_file_size(&self) -> Result<u64> {
        let metadata = tokio::fs::metadata(&self.file_path).await.map_err(|e| {
            RustyPotatoError::PersistenceError {
                message: format!("Failed to stat AOF: {e}"),
                source: Some(e),
                recoverable: true,
            }
        })?;
        Ok(metadata.len() + self.buffer.len() as u64)
    }

    /// Atomically replace the current AOF file with `temp_path`'s
    /// contents. The previous file (and any pending buffer contents)
    /// are discarded — the caller is responsible for ensuring the
    /// rewriter's snapshot already contains the relevant state.
    ///
    /// The Unix rename is atomic by POSIX. On Windows, `std::fs::rename`
    /// (and tokio's wrapper) maps to `MoveFileExW(MOVEFILE_REPLACE_EXISTING)`
    /// which is "mostly" atomic — there's a narrow window where a crash
    /// between the unlink and the rename could leave the destination
    /// missing. Documented as a known limitation; real Redis on Windows
    /// has the same constraint.
    async fn swap_file(&mut self, temp_path: PathBuf) -> Result<()> {
        // Flush any buffered writes that haven't hit disk yet — these
        // were destined for the OLD file. Per the contract, the caller
        // is expected to hold a write barrier, so this buffer should
        // be empty in practice. Drop it without writing to be safe.
        self.buffer.clear();

        // Drop the old file handle BEFORE the rename so Windows can
        // unlink the destination. POSIX is fine either way.
        let placeholder = OpenOptions::new()
            .read(true)
            .open(&temp_path)
            .await
            .map_err(|e| RustyPotatoError::PersistenceError {
                message: format!("Cannot open temp AOF {}: {e}", temp_path.display()),
                source: Some(e),
                recoverable: false,
            })?;
        // We swap `self.file` into `placeholder` to drop the old handle.
        let _ = std::mem::replace(&mut self.file, placeholder);

        tokio::fs::rename(&temp_path, &self.file_path)
            .await
            .map_err(|e| RustyPotatoError::PersistenceError {
                message: format!(
                    "Atomic rename {} → {} failed: {e}",
                    temp_path.display(),
                    self.file_path.display()
                ),
                source: Some(e),
                recoverable: false,
            })?;

        // Reopen in append mode so subsequent records go to the new file.
        let new_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.file_path)
            .await
            .map_err(|e| RustyPotatoError::PersistenceError {
                message: format!("Cannot reopen AOF {}: {e}", self.file_path.display()),
                source: Some(e),
                recoverable: false,
            })?;
        self.file = new_file;
        self.last_flush = Instant::now();

        info!(
            "AOF rewrite committed: swapped to {}",
            self.file_path.display()
        );
        Ok(())
    }
}

/// Coordinates AOF logging from the rest of the system.
///
/// All disk I/O happens in a single background task; the manager itself is
/// just a handle that forwards messages over a bounded channel. There is
/// exactly one writer per AOF file, so flush/file-size/shutdown all see a
/// consistent view of state — fixing the previous "two writers on one file"
/// bug.
#[derive(Debug)]
pub struct PersistenceManager {
    sender: Option<mpsc::Sender<WriterCmd>>,
    aof_path: PathBuf,
    enabled: bool,
}

impl PersistenceManager {
    /// Construct a manager. If AOF is disabled in config the manager is a
    /// no-op shim that accepts log calls and drops them.
    pub async fn new(config: &Config) -> Result<Self> {
        let aof_path = config.storage.aof_path.clone();

        if !config.storage.aof_enabled {
            return Ok(Self {
                sender: None,
                aof_path,
                enabled: false,
            });
        }

        let (sender, receiver) = mpsc::channel(AOF_CHANNEL_CAPACITY);
        let writer = AofWriter::open(config).await?;
        tokio::spawn(run_writer_task(writer, receiver));

        Ok(Self {
            sender: Some(sender),
            aof_path,
            enabled: true,
        })
    }

    /// Append a RESP-encoded command frame to the AOF. Blocks the caller if
    /// the writer falls behind (backpressure).
    pub async fn log_command(&self, bytes: Bytes) -> Result<()> {
        let Some(sender) = &self.sender else {
            return Ok(());
        };
        sender
            .send(WriterCmd::Record(bytes))
            .await
            .map_err(channel_closed)
    }

    /// Force a flush + fsync; awaits completion.
    pub async fn flush(&self) -> Result<()> {
        let Some(sender) = &self.sender else {
            return Ok(());
        };
        let (tx, rx) = oneshot::channel();
        sender
            .send(WriterCmd::Flush(tx))
            .await
            .map_err(channel_closed)?;
        rx.await.map_err(reply_dropped)?
    }

    /// Current AOF file size on disk (plus any unflushed buffer).
    pub async fn file_size(&self) -> Result<u64> {
        let Some(sender) = &self.sender else {
            return Ok(0);
        };
        let (tx, rx) = oneshot::channel();
        sender
            .send(WriterCmd::FileSize(tx))
            .await
            .map_err(channel_closed)?;
        rx.await.map_err(reply_dropped)?
    }

    /// Shut down the writer task, performing a final flush.
    pub async fn shutdown(&self) -> Result<()> {
        let Some(sender) = &self.sender else {
            return Ok(());
        };
        let (tx, rx) = oneshot::channel();
        sender
            .send(WriterCmd::Shutdown(tx))
            .await
            .map_err(channel_closed)?;
        rx.await.map_err(reply_dropped)?
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn aof_path(&self) -> &Path {
        &self.aof_path
    }

    /// Compact the AOF by writing `frames` (a sequence of RESP-encoded
    /// commands representing the live store snapshot) to a temp file
    /// and atomically renaming it over the live AOF.
    ///
    /// **Caller contract**: hold a write barrier preventing new
    /// mutations from being dispatched while this is running. The
    /// `MemoryStore::snapshot` that produced these frames is only
    /// consistent if no mutations ran between snapshot capture and
    /// commit; otherwise the swap discards those mutations along with
    /// the old file.
    pub async fn rewrite_aof(&self, frames: Vec<Bytes>) -> Result<()> {
        let Some(sender) = &self.sender else {
            return Ok(()); // AOF disabled — no-op.
        };

        // Write all frames to a sibling temp file. Suffix encodes the
        // PID so concurrent rewrites (which we don't allow but which
        // a misbehaving caller might attempt) don't collide.
        let temp_path = self
            .aof_path
            .with_extension(format!("rewrite.{}.tmp", std::process::id()));

        if let Some(parent) = temp_path.parent() {
            if !parent.as_os_str().is_empty() {
                tokio::fs::create_dir_all(parent).await.map_err(|e| {
                    RustyPotatoError::PersistenceError {
                        message: format!(
                            "Cannot ensure rewrite-temp dir {}: {e}",
                            parent.display()
                        ),
                        source: Some(e),
                        recoverable: false,
                    }
                })?;
            }
        }

        let mut temp = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&temp_path)
            .await
            .map_err(|e| RustyPotatoError::PersistenceError {
                message: format!("Cannot open rewrite temp {}: {e}", temp_path.display()),
                source: Some(e),
                recoverable: false,
            })?;

        for frame in &frames {
            temp.write_all(frame)
                .await
                .map_err(|e| RustyPotatoError::PersistenceError {
                    message: format!("Rewrite write failed: {e}"),
                    source: Some(e),
                    recoverable: true,
                })?;
        }
        temp.flush()
            .await
            .map_err(|e| RustyPotatoError::PersistenceError {
                message: format!("Rewrite flush failed: {e}"),
                source: Some(e),
                recoverable: true,
            })?;
        temp.sync_all()
            .await
            .map_err(|e| RustyPotatoError::PersistenceError {
                message: format!("Rewrite fsync failed: {e}"),
                source: Some(e),
                recoverable: true,
            })?;
        // Drop the handle so the writer task can rename the file out.
        drop(temp);

        // Hand the temp path to the writer task for the atomic swap.
        let (tx, rx) = oneshot::channel();
        sender
            .send(WriterCmd::SwapFile {
                temp_path: temp_path.clone(),
                reply: tx,
            })
            .await
            .map_err(channel_closed)?;
        match rx.await.map_err(reply_dropped)? {
            Ok(()) => Ok(()),
            Err(e) => {
                // Best-effort cleanup of the temp file on failure.
                let _ = tokio::fs::remove_file(&temp_path).await;
                Err(e)
            }
        }
    }
}

fn channel_closed(_: mpsc::error::SendError<WriterCmd>) -> RustyPotatoError {
    RustyPotatoError::PersistenceError {
        message: "AOF writer task is no longer accepting messages".to_string(),
        source: Some(std::io::Error::new(
            std::io::ErrorKind::BrokenPipe,
            "AOF writer task gone",
        )),
        recoverable: false,
    }
}

fn reply_dropped(_: oneshot::error::RecvError) -> RustyPotatoError {
    RustyPotatoError::PersistenceError {
        message: "AOF writer dropped a reply channel".to_string(),
        source: Some(std::io::Error::new(
            std::io::ErrorKind::BrokenPipe,
            "AOF writer reply lost",
        )),
        recoverable: false,
    }
}

/// The single AOF writer task. Owns the file handle exclusively.
async fn run_writer_task(mut writer: AofWriter, mut receiver: mpsc::Receiver<WriterCmd>) {
    let mut tick = interval(Duration::from_secs(1));

    loop {
        tokio::select! {
            msg = receiver.recv() => match msg {
                Some(WriterCmd::Record(bytes)) => {
                    if let Err(e) = writer.record(bytes).await {
                        error!("AOF record failed: {e}");
                    }
                }
                Some(WriterCmd::Flush(reply)) => {
                    let result = writer.flush_to_disk(true).await;
                    let _ = reply.send(result);
                }
                Some(WriterCmd::FileSize(reply)) => {
                    let result = writer.current_file_size().await;
                    let _ = reply.send(result);
                }
                Some(WriterCmd::SwapFile { temp_path, reply }) => {
                    let result = writer.swap_file(temp_path).await;
                    let _ = reply.send(result);
                }
                Some(WriterCmd::Shutdown(reply)) => {
                    let result = writer.flush_to_disk(true).await;
                    let _ = reply.send(result);
                    debug!("AOF writer task shutting down");
                    break;
                }
                None => {
                    // Senders all dropped — flush and exit.
                    if let Err(e) = writer.flush_to_disk(true).await {
                        error!("Final AOF flush failed: {e}");
                    }
                    break;
                }
            },
            _ = tick.tick() => {
                if let Err(e) = writer.periodic_flush().await {
                    error!("Periodic AOF flush failed: {e}");
                }
            }
        }
    }
}

/// Stream the AOF file, parse RESP frames, and call `replay_fn` for each
/// command. Returns the number of commands replayed.
///
/// The replay function is responsible for executing the command against
/// whatever store the caller controls. Errors from `replay_fn` are logged
/// and recovery continues — partial AOF corruption produces a warning, not
/// an abort.
pub async fn replay_aof_file<F, Fut>(path: &Path, mut replay_fn: F) -> Result<usize>
where
    F: FnMut(crate::commands::ParsedCommand) -> Fut,
    Fut: std::future::Future<Output = Result<()>>,
{
    if !path.exists() {
        info!(
            "No AOF file at {}, starting with empty database",
            path.display()
        );
        return Ok(0);
    }

    info!("Beginning AOF recovery from {}", path.display());
    let started = Instant::now();

    let mut file = File::open(path)
        .await
        .map_err(|e| RustyPotatoError::PersistenceError {
            message: format!("Cannot open AOF {}: {e}", path.display()),
            source: Some(e),
            recoverable: false,
        })?;

    let mut codec = RespCodec::new();
    let mut chunk = vec![0u8; 64 * 1024];
    let mut replayed = 0usize;

    loop {
        let n = file
            .read(&mut chunk)
            .await
            .map_err(|e| RustyPotatoError::PersistenceError {
                message: format!("AOF read failed: {e}"),
                source: Some(e),
                recoverable: true,
            })?;

        // Drain any commands already buffered in the codec from previous
        // chunks before checking for EOF.
        let data = if n == 0 { &[] } else { &chunk[..n] };

        match codec.decode(data) {
            Ok(Some(cmd)) => {
                if let Err(e) = replay_fn(cmd).await {
                    error!("AOF replay failure: {e}");
                }
                replayed += 1;
                // Drain remaining commands in the buffer with empty input.
                while let Ok(Some(more)) = codec.decode(&[]) {
                    if let Err(e) = replay_fn(more).await {
                        error!("AOF replay failure: {e}");
                    }
                    replayed += 1;
                }
            }
            Ok(None) => {
                if n == 0 {
                    break;
                }
            }
            Err(e) => {
                error!("AOF parse error (truncated tail?): {e} — stopping replay");
                break;
            }
        }
    }

    info!(
        "AOF recovery completed: {} commands replayed in {:?}",
        replayed,
        started.elapsed()
    );
    Ok(replayed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::ResponseValue;
    use crate::config::{Config, StorageConfig};
    use crate::network::protocol::RespCodec;
    use bytes::BufMut;
    use tempfile::tempdir;

    fn enabled_config(path: PathBuf) -> Config {
        Config {
            storage: StorageConfig {
                aof_enabled: true,
                aof_path: path,
                aof_fsync_policy: FsyncPolicy::Always,
            },
            ..Default::default()
        }
    }

    /// Encode a SET command as RESP frame bytes for tests.
    fn resp_frame_set(key: &str, value: &str) -> Bytes {
        let mut buf = bytes::BytesMut::new();
        buf.put_slice(b"*3\r\n$3\r\nSET\r\n");
        buf.put_slice(format!("${}\r\n", key.len()).as_bytes());
        buf.put_slice(key.as_bytes());
        buf.put_slice(b"\r\n");
        buf.put_slice(format!("${}\r\n", value.len()).as_bytes());
        buf.put_slice(value.as_bytes());
        buf.put_slice(b"\r\n");
        buf.freeze()
    }

    #[tokio::test]
    async fn writer_appends_resp_frames_and_recovers_them() {
        let dir = tempdir().unwrap();
        let aof = dir.path().join("test.aof");
        let cfg = enabled_config(aof.clone());

        let mgr = PersistenceManager::new(&cfg).await.unwrap();
        assert!(mgr.is_enabled());

        mgr.log_command(resp_frame_set("foo", "bar")).await.unwrap();
        mgr.log_command(resp_frame_set("baz", "qux")).await.unwrap();
        mgr.flush().await.unwrap();
        mgr.shutdown().await.unwrap();

        // Verify by re-reading the file directly with the codec.
        let mut codec = RespCodec::new();
        let bytes = tokio::fs::read(&aof).await.unwrap();
        let cmd1 = codec.decode(&bytes).unwrap().expect("cmd1");
        assert_eq!(cmd1.name, "SET");
        assert_eq!(cmd1.args, vec!["foo", "bar"]);
        let cmd2 = codec.decode(&[]).unwrap().expect("cmd2");
        assert_eq!(cmd2.name, "SET");
        assert_eq!(cmd2.args, vec!["baz", "qux"]);
    }

    #[tokio::test]
    async fn replay_invokes_callback_for_each_command() {
        let dir = tempdir().unwrap();
        let aof = dir.path().join("test.aof");
        let cfg = enabled_config(aof.clone());

        let mgr = PersistenceManager::new(&cfg).await.unwrap();
        for i in 0..5 {
            let key = format!("key{i}");
            mgr.log_command(resp_frame_set(&key, "v")).await.unwrap();
        }
        mgr.flush().await.unwrap();
        mgr.shutdown().await.unwrap();

        let mut keys_seen: Vec<String> = vec![];
        let count = replay_aof_file(&aof, |cmd| {
            keys_seen.push(String::from_utf8_lossy(&cmd.args[0]).into_owned());
            async { Ok(()) }
        })
        .await
        .unwrap();

        assert_eq!(count, 5);
        assert_eq!(keys_seen, vec!["key0", "key1", "key2", "key3", "key4"]);
    }

    #[tokio::test]
    async fn replay_on_missing_file_returns_zero() {
        let dir = tempdir().unwrap();
        let aof = dir.path().join("does_not_exist.aof");

        let count = replay_aof_file(&aof, |_| async { Ok(()) }).await.unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn disabled_manager_is_a_noop() {
        let dir = tempdir().unwrap();
        let aof = dir.path().join("disabled.aof");
        let mut cfg = enabled_config(aof.clone());
        cfg.storage.aof_enabled = false;

        let mgr = PersistenceManager::new(&cfg).await.unwrap();
        assert!(!mgr.is_enabled());
        mgr.log_command(resp_frame_set("x", "y")).await.unwrap();
        mgr.flush().await.unwrap();
        assert_eq!(mgr.file_size().await.unwrap(), 0);
        assert!(!aof.exists(), "no file created when disabled");
    }

    // Silence unused-import warning when only some tests reference it.
    #[allow(dead_code)]
    fn _touch(_: ResponseValue) {}
}
