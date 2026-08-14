use crate::auth::{AccountSecrets, AuthEndpoints, TokenPersist, ensure_mc_token_owned, secret_id};
use crate::error::EngineError;
use crate::fabric::{
    FabricLoaderIndex, FabricProfile, LOADER_INDEX_URL, merge_fabric, pick_loader_version,
    profile_url,
};
use crate::forge::{merge_forge, prepare_forge, run_processors};
use crate::http::HttpFiles;
use crate::ids::{AccountId, InstanceId, Loader};
use crate::instance_not_found;
use crate::java::resolve_java;
use crate::mojang::{
    ArgContext, AssetsRoot, FeatureSet, VersionInfo, build_args, extract_natives, fetch_assets,
    fetch_client, fetch_libraries, join_classpath, natives_dir_name, select_libraries,
};
use crate::now_ms;
use crate::redact::redact_line_with_tokens;
use crate::store::Store;
use crate::types::{
    AccountRecord, GameProcessId, InstanceRow, LaunchPlan, ProgressSink, QuickPlay, SandboxSpec,
};
use crate::{Engine, Event, LogStream, Running};
use chrono::Utc;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio_util::sync::CancellationToken;

pub const VERSION_MANIFEST_URL: &str =
    "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";

struct PreparingGuard<'a> {
    preparing: &'a parking_lot::Mutex<HashSet<InstanceId>>,
    id: InstanceId,
}

impl Drop for PreparingGuard<'_> {
    fn drop(&mut self) {
        self.preparing.lock().remove(&self.id);
    }
}

impl Engine {
    pub async fn prepare(
        &self,
        id: InstanceId,
        progress: &dyn ProgressSink,
        cancel: CancellationToken,
        quick_play: Option<QuickPlay>,
    ) -> Result<LaunchPlan, EngineError> {
        let result = self
            .prepare_vanilla(id, progress, &cancel, quick_play)
            .await;
        self.emit(Event::PrepareFinished {
            id,
            ok: result.is_ok(),
        });
        result
    }

    pub fn spawn(&self, id: InstanceId, plan: LaunchPlan) -> Result<GameProcessId, EngineError> {
        if plan.sandbox.enabled {
            if let crate::types::SandboxStatus::Unavailable { reason } =
                crate::sandbox::sandbox_status()
            {
                return Err(EngineError::SandboxUnavailable { reason });
            }
        }
        if self.processes.lock().contains_key(&id) {
            return Err(EngineError::InstanceBusy);
        }

        let tokens = self.redact_tokens.lock().remove(&id).unwrap_or_default();
        let (kill_tx, kill_rx) = tokio::sync::watch::channel(false);
        self.processes.lock().insert(
            id,
            Running {
                kill: kill_tx,
                started_at: Instant::now(),
            },
        );

        let events = self.events.clone();
        let processes = Arc::clone(&self.processes);
        let db = self.paths.db.clone();

        if plan.sandbox.enabled {
            let mut plan = plan;
            plan.sandbox = crate::sandbox::fill_spec(&plan, &self.paths);
            let child = match crate::sandbox::spawn_sandboxed(&plan) {
                Ok(child) => child,
                Err(err) => {
                    self.processes.lock().remove(&id);
                    return Err(err);
                }
            };
            let pid = child.id();
            self.rt.spawn(async move {
                watch_std_process(child, kill_rx, id, tokens, events, processes, db).await;
            });
            return Ok(GameProcessId(pid));
        }

        let mut cmd = tokio::process::Command::new(&plan.java);
        cmd.args(&plan.jvm_args)
            .arg(&plan.main_class)
            .args(&plan.game_args)
            .current_dir(&plan.cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(false);
        for (key, value) in &plan.env {
            cmd.env(key, value);
        }

        let mut child = match cmd.spawn() {
            Ok(child) => child,
            Err(e) => {
                self.processes.lock().remove(&id);
                return Err(EngineError::io(&plan.java, e));
            }
        };
        let pid = child.id().unwrap_or(0);
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        self.rt.spawn(async move {
            watch_process(
                child, stdout, stderr, kill_rx, id, tokens, events, processes, db,
            )
            .await;
        });
        Ok(GameProcessId(pid))
    }

    pub fn kill(&self, id: InstanceId) -> Result<(), EngineError> {
        if let Some(running) = self.processes.lock().get(&id) {
            let _ = running.kill.send(true);
        }
        Ok(())
    }

    async fn prepare_vanilla(
        &self,
        id: InstanceId,
        progress: &dyn ProgressSink,
        cancel: &CancellationToken,
        quick_play: Option<QuickPlay>,
    ) -> Result<LaunchPlan, EngineError> {
        check_cancel(cancel)?;
        let _guard = self.begin_prepare(id)?;

        check_cancel(cancel)?;
        let row = {
            let store = self.store.lock();
            store
                .get_instance(id)?
                .ok_or_else(|| instance_not_found(&self.paths))?
        };

        check_cancel(cancel)?;
        let account = {
            let store = self.store.lock();
            resolve_account(&store, &row)?
        };
        let Some(account) = account else {
            self.emit(Event::AuthRequired);
            return Err(EngineError::NoAccount);
        };

        check_cancel(cancel)?;
        let http = HttpFiles::new()?;
        let sid = secret_id(account.uuid);
        let secrets = {
            let token_store = Store::open_file(&self.paths.db)?;
            load_account_secrets(&token_store, &self.master_key, &sid)?
        };
        let (mc_token, persist) =
            ensure_mc_token_owned(&http, secrets, Utc::now(), &AuthEndpoints::production()).await?;
        {
            let token_store = Store::open_file(&self.paths.db)?;
            match persist {
                TokenPersist::Unchanged => {}
                TokenPersist::Save(secrets) => {
                    save_account_secrets(&token_store, &self.master_key, &sid, &secrets)?;
                }
                TokenPersist::Delete => {
                    token_store.delete_secret(&sid)?;
                    return Err(EngineError::AuthExpired);
                }
            }
        }
        let tokens = {
            let token_store = Store::open_file(&self.paths.db)?;
            collect_redact_tokens(&token_store, &self.master_key, account.uuid, &mc_token)
        };
        self.redact_tokens.lock().insert(id, tokens);

        check_cancel(cancel)?;
        progress.set("Version manifest", 0, 1);
        let manifest_path = self.paths.cache_meta.join("version_manifest_v2.json");
        if manifest_path.exists() {
            let _ = std::fs::remove_file(&manifest_path);
        }
        http.download_sha1(VERSION_MANIFEST_URL, &manifest_path, None, cancel)
            .await?;
        progress.set("Version manifest", 1, 1);
        let manifest: VersionManifest = read_json(&manifest_path)?;
        let entry = manifest
            .versions
            .iter()
            .find(|v| v.id == row.minecraft_version)
            .ok_or_else(|| EngineError::VersionNotFound {
                id: row.minecraft_version.clone(),
            })?;

        check_cancel(cancel)?;
        progress.set("Version", 0, 1);
        let version_path = self.paths.cache_meta.join(meta_json_name(&entry.id)?);
        http.download_sha1(&entry.url, &version_path, entry.sha1.as_deref(), cancel)
            .await?;
        progress.set("Version", 1, 1);
        let version: VersionInfo = read_json(&version_path)?;
        let mut version = if row.loader == Loader::Fabric {
            merge_fabric_profile(&http, &row, version, progress, cancel).await?
        } else {
            version
        };
        let forge = if row.loader == Loader::Forge {
            Some(
                prepare_forge(
                    &http,
                    &self.paths,
                    &row.minecraft_version,
                    row.loader_version.as_deref(),
                    progress,
                    cancel,
                )
                .await?,
            )
        } else {
            None
        };

        check_cancel(cancel)?;
        let custom = row.java_path.as_ref().map(PathBuf::from);
        let java = resolve_java(
            &http,
            &self.paths,
            &version,
            custom.as_deref(),
            progress,
            cancel,
        )
        .await?;

        check_cancel(cancel)?;
        progress.set("Client", 0, 1);
        let client = fetch_client(&http, &self.paths, &version, cancel).await?;
        progress.set("Client", 1, 1);

        if let Some((profile, forge_version)) = forge {
            check_cancel(cancel)?;
            run_processors(&java, &profile, &self.paths, &client, cancel).await?;
            version = merge_forge(version, forge_version);
        }

        check_cancel(cancel)?;
        let artifacts = select_libraries(&version);
        let lib_dests = fetch_libraries(&http, &self.paths, &artifacts, progress, cancel).await?;

        check_cancel(cancel)?;
        let natives_dir = self
            .paths
            .cache_natives
            .join(natives_dir_name(&artifacts, row.sandbox));
        std::fs::create_dir_all(&natives_dir).map_err(|e| EngineError::io(&natives_dir, e))?;
        let native_count = artifacts.iter().filter(|a| a.extract_natives).count() as u64;
        let mut natives_done = 0u64;
        if native_count == 0 {
            progress.set("Natives", 0, 0);
        }
        for artifact in artifacts.iter().filter(|a| a.extract_natives) {
            check_cancel(cancel)?;
            let jar = self.paths.cache_libraries.join(&artifact.path);
            let exclude = exclude_for(&version, &artifact.path);
            extract_natives(&jar, &natives_dir, &exclude)?;
            natives_done += 1;
            progress.set("Natives", natives_done, native_count);
        }

        let cwd = self.paths.instance_minecraft(&row.slug);
        std::fs::create_dir_all(&cwd).map_err(|e| EngineError::io(&cwd, e))?;

        check_cancel(cancel)?;
        let (assets_root, assets_index_name) = if let Some(idx) = &version.asset_index {
            let root = fetch_assets(
                &http,
                &self.paths,
                &idx.url,
                &idx.sha1,
                &idx.id,
                &cwd,
                progress,
                cancel,
            )
            .await?;
            (assets_root_path(&root), idx.id.clone())
        } else {
            (
                self.paths
                    .cache_assets_objects
                    .parent()
                    .unwrap_or(&self.paths.cache_assets_objects)
                    .to_path_buf(),
                version.assets.clone().unwrap_or_else(|| "legacy".into()),
            )
        };

        check_cancel(cancel)?;
        let logging_arg =
            if let Some(client_log) = version.logging.as_ref().and_then(|l| l.client.as_ref()) {
                let dest = self
                    .paths
                    .cache_meta
                    .join("logconfigs")
                    .join(&client_log.file.id);
                progress.set("Logging", 0, 1);
                http.download_sha1(
                    &client_log.file.url,
                    &dest,
                    Some(client_log.file.sha1.as_str()),
                    cancel,
                )
                .await?;
                progress.set("Logging", 1, 1);
                Some(
                    client_log
                        .argument
                        .replace("${path}", &dest.to_string_lossy()),
                )
            } else {
                None
            };

        let mut classpath = Vec::new();
        for (artifact, dest) in artifacts.iter().zip(lib_dests) {
            if !artifact.extract_natives {
                classpath.push(dest);
            }
        }
        classpath.push(client);

        let (features, quick_play_singleplayer, quick_play_multiplayer) =
            quick_play_launch(quick_play);
        let ctx = ArgContext {
            auth_player_name: account.username,
            auth_uuid: account.uuid.0.as_simple().to_string(),
            auth_access_token: mc_token,
            user_type: "msa".into(),
            version_name: version.id.clone(),
            game_directory: cwd.to_string_lossy().into_owned(),
            assets_root: assets_root.to_string_lossy().into_owned(),
            assets_index_name,
            natives_directory: natives_dir.to_string_lossy().into_owned(),
            launcher_name: "kmine".into(),
            launcher_version: env!("CARGO_PKG_VERSION").into(),
            classpath: join_classpath(&classpath),
            library_directory: self.paths.cache_libraries.to_string_lossy().into_owned(),
            resolution_width: "854".into(),
            resolution_height: "480".into(),
            quick_play_singleplayer,
            quick_play_multiplayer,
        };
        let (mut jvm_args, game_args) = build_args(&version, &ctx, &features);
        if let Some(arg) = logging_arg {
            jvm_args.push(arg);
        }
        apply_memory_and_flags(&mut jvm_args, &row)?;

        let mut plan = LaunchPlan {
            java: java.clone(),
            jvm_args,
            main_class: version.main_class,
            game_args,
            classpath,
            natives_dir: natives_dir.clone(),
            cwd: cwd.clone(),
            env: Vec::new(),
            sandbox: SandboxSpec {
                enabled: row.sandbox,
                allow_read: Vec::new(),
                allow_write: Vec::new(),
                network: true,
            },
        };
        plan.sandbox = crate::sandbox::fill_spec(&plan, &self.paths);
        Ok(plan)
    }

    fn begin_prepare(&self, id: InstanceId) -> Result<PreparingGuard<'_>, EngineError> {
        let mut preparing = self.preparing.lock();
        let processes = self.processes.lock();
        if processes.contains_key(&id) || preparing.contains(&id) {
            return Err(EngineError::InstanceBusy);
        }
        preparing.insert(id);
        Ok(PreparingGuard {
            preparing: &self.preparing,
            id,
        })
    }
}

async fn merge_fabric_profile(
    http: &HttpFiles,
    row: &InstanceRow,
    vanilla: VersionInfo,
    progress: &dyn ProgressSink,
    cancel: &CancellationToken,
) -> Result<VersionInfo, EngineError> {
    check_cancel(cancel)?;
    progress.set("Fabric loader", 0, 2);
    let index: FabricLoaderIndex = http.get_json(LOADER_INDEX_URL, cancel).await?;
    let loader = match pick_loader_version(&index, row.loader_version.as_deref()) {
        Ok(version) => version,
        Err(EngineError::LoaderUnavailable { loader, .. }) => {
            return Err(EngineError::LoaderUnavailable {
                loader,
                minecraft: row.minecraft_version.clone(),
            });
        }
        Err(err) => return Err(err),
    };
    progress.set("Fabric loader", 1, 2);
    check_cancel(cancel)?;
    let url = profile_url(&row.minecraft_version, &loader);
    let profile: FabricProfile = match http.get_json(&url, cancel).await {
        Ok(profile) => profile,
        Err(EngineError::Http { status: 404, .. }) => {
            return Err(EngineError::LoaderUnavailable {
                loader: Loader::Fabric,
                minecraft: row.minecraft_version.clone(),
            });
        }
        Err(err) => return Err(err),
    };
    progress.set("Fabric loader", 2, 2);
    Ok(merge_fabric(vanilla, profile))
}

fn quick_play_launch(
    quick_play: Option<QuickPlay>,
) -> (FeatureSet, Option<String>, Option<String>) {
    let mut features = FeatureSet::default();
    match quick_play {
        Some(QuickPlay::World { folder }) => {
            features.quick_play_single = true;
            (features, Some(folder), None)
        }
        Some(QuickPlay::Server { address }) => {
            features.quick_play_multi = true;
            (features, None, Some(address))
        }
        None => (features, None, None),
    }
}

fn check_cancel(cancel: &CancellationToken) -> Result<(), EngineError> {
    if cancel.is_cancelled() {
        Err(EngineError::Cancelled)
    } else {
        Ok(())
    }
}

fn resolve_account(store: &Store, row: &InstanceRow) -> Result<Option<AccountRecord>, EngineError> {
    let uuid = match row.account_uuid {
        Some(id) => Some(id),
        None => store.selected_account()?,
    };
    let Some(uuid) = uuid else {
        return Ok(None);
    };
    Ok(store
        .list_accounts()?
        .into_iter()
        .find(|account| account.uuid == uuid))
}

fn load_account_secrets(
    store: &Store,
    key: &[u8; 32],
    sid: &str,
) -> Result<AccountSecrets, EngineError> {
    let raw = match store.get_secret(key, sid) {
        Ok(Some(bytes)) => bytes,
        Ok(None) => return Err(EngineError::AuthExpired),
        Err(EngineError::Crypto) => return Err(EngineError::AuthExpired),
        Err(err) => return Err(err),
    };
    serde_json::from_slice(&raw).map_err(|_| EngineError::AuthExpired)
}

fn save_account_secrets(
    store: &Store,
    key: &[u8; 32],
    sid: &str,
    secrets: &AccountSecrets,
) -> Result<(), EngineError> {
    let bytes = serde_json::to_vec(secrets).map_err(|_| EngineError::AuthFailed {
        message: "failed to encode secrets".into(),
    })?;
    store.put_secret(key, sid, &bytes)
}

fn collect_redact_tokens(
    store: &Store,
    key: &[u8; 32],
    account: AccountId,
    mc_token: &str,
) -> Vec<String> {
    let mut tokens = Vec::new();
    if !mc_token.is_empty() {
        tokens.push(mc_token.to_string());
    }
    let Ok(Some(bytes)) = store.get_secret(key, &secret_id(account)) else {
        return tokens;
    };
    let Ok(secrets) = serde_json::from_slice::<AccountSecrets>(&bytes) else {
        return tokens;
    };
    if let Some(token) = secrets.msa_refresh.filter(|t| !t.is_empty()) {
        tokens.push(token);
    }
    if let Some(token) = secrets
        .msa_access
        .map(|t| t.token)
        .filter(|t| !t.is_empty())
    {
        tokens.push(token);
    }
    if let Some(token) = secrets.xbl.map(|t| t.token).filter(|t| !t.is_empty()) {
        tokens.push(token);
    }
    if let Some(token) = secrets.xsts.map(|t| t.token).filter(|t| !t.is_empty()) {
        tokens.push(token);
    }
    tokens
}

fn apply_memory_and_flags(
    jvm_args: &mut Vec<String>,
    row: &InstanceRow,
) -> Result<(), EngineError> {
    if let Some(min) = row.memory_min_mb {
        jvm_args.insert(0, format!("-Xms{min}M"));
    }
    if let Some(max) = row.memory_max_mb {
        let idx = usize::from(row.memory_min_mb.is_some());
        jvm_args.insert(idx, format!("-Xmx{max}M"));
    }
    if let Some(flags) = row.jvm_flags.as_deref().filter(|s| !s.is_empty()) {
        let extra = shell_words::split(flags).map_err(|e| {
            EngineError::io(
                PathBuf::from("jvm_flags"),
                io::Error::new(io::ErrorKind::InvalidInput, e.to_string()),
            )
        })?;
        jvm_args.extend(extra);
    }
    Ok(())
}

fn assets_root_path(root: &AssetsRoot) -> PathBuf {
    match root {
        AssetsRoot::Objects { dir, .. } => dir.parent().unwrap_or(dir).to_path_buf(),
        AssetsRoot::Virtual(path) | AssetsRoot::Resources(path) => path.clone(),
    }
}

fn exclude_for(version: &VersionInfo, path: &str) -> Vec<String> {
    for lib in &version.libraries {
        let Some(downloads) = &lib.downloads else {
            continue;
        };
        let hit = downloads
            .classifiers
            .as_ref()
            .is_some_and(|c| c.values().any(|a| a.path.as_deref() == Some(path)));
        if hit {
            return lib
                .extract
                .as_ref()
                .map(|e| e.exclude.clone())
                .unwrap_or_default();
        }
    }
    Vec::new()
}

fn meta_json_name(id: &str) -> Result<String, EngineError> {
    if id.is_empty() || id.contains('/') || id.contains('\\') {
        return Err(EngineError::VersionNotFound { id: id.into() });
    }
    Ok(format!("{id}.json"))
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T, EngineError> {
    let bytes = std::fs::read(path).map_err(|e| EngineError::io(path, e))?;
    serde_json::from_slice(&bytes)
        .map_err(|e| EngineError::io(path, io::Error::other(e.to_string())))
}

async fn watch_process(
    mut child: tokio::process::Child,
    stdout: Option<tokio::process::ChildStdout>,
    stderr: Option<tokio::process::ChildStderr>,
    mut kill_rx: tokio::sync::watch::Receiver<bool>,
    id: InstanceId,
    tokens: Vec<String>,
    events: tokio::sync::broadcast::Sender<Event>,
    processes: Arc<parking_lot::Mutex<std::collections::HashMap<InstanceId, Running>>>,
    db: PathBuf,
) {
    if let Some(out) = stdout {
        let events = events.clone();
        let tokens = tokens.clone();
        tokio::spawn(async move {
            pump_lines(out, LogStream::Stdout, id, &events, &tokens).await;
        });
    }
    if let Some(err) = stderr {
        let events = events.clone();
        let tokens = tokens.clone();
        tokio::spawn(async move {
            pump_lines(err, LogStream::Stderr, id, &events, &tokens).await;
        });
    }

    let status = loop {
        tokio::select! {
            status = child.wait() => break status,
            changed = kill_rx.changed() => {
                if changed.is_err() || *kill_rx.borrow() {
                    let _ = child.start_kill();
                }
            }
        }
    };

    let elapsed = {
        let mut procs = processes.lock();
        let started = procs
            .get(&id)
            .map(|r| r.started_at)
            .unwrap_or_else(Instant::now);
        procs.remove(&id);
        started.elapsed()
    };
    record_session(&db, id, elapsed);
    let code = status.ok().and_then(|s| s.code());
    let _ = events.send(Event::ProcessExited {
        instance_id: id,
        code,
    });
    let _ = events.send(Event::InstancesChanged);
}

async fn watch_std_process(
    mut child: std::process::Child,
    mut kill_rx: tokio::sync::watch::Receiver<bool>,
    id: InstanceId,
    tokens: Vec<String>,
    events: tokio::sync::broadcast::Sender<Event>,
    processes: Arc<parking_lot::Mutex<std::collections::HashMap<InstanceId, Running>>>,
    db: PathBuf,
) {
    if let Some(out) = child.stdout.take() {
        pump_std_lines(out, LogStream::Stdout, id, events.clone(), tokens.clone());
    }
    if let Some(err) = child.stderr.take() {
        pump_std_lines(err, LogStream::Stderr, id, events.clone(), tokens.clone());
    }

    let pid = child.id();
    if *kill_rx.borrow() {
        kill_pid(pid);
    }
    let mut wait = tokio::task::spawn_blocking(move || child.wait());
    let status = loop {
        tokio::select! {
            res = &mut wait => break res.ok().and_then(Result::ok),
            changed = kill_rx.changed() => {
                if changed.is_err() || *kill_rx.borrow() {
                    kill_pid(pid);
                }
            }
        }
    };

    let elapsed = {
        let mut procs = processes.lock();
        let started = procs
            .get(&id)
            .map(|r| r.started_at)
            .unwrap_or_else(Instant::now);
        procs.remove(&id);
        started.elapsed()
    };
    record_session(&db, id, elapsed);
    let code = status.and_then(|s| s.code());
    let _ = events.send(Event::ProcessExited {
        instance_id: id,
        code,
    });
    let _ = events.send(Event::InstancesChanged);
}

fn pump_std_lines(
    out: impl io::Read + Send + 'static,
    stream: LogStream,
    id: InstanceId,
    events: tokio::sync::broadcast::Sender<Event>,
    tokens: Vec<String>,
) {
    tokio::task::spawn_blocking(move || {
        use io::BufRead;
        let reader = io::BufReader::new(out);
        for line in reader.lines() {
            let Ok(line) = line else {
                break;
            };
            let text = redact_line_with_tokens(&line, &tokens);
            let _ = events.send(Event::LogLine {
                instance_id: id,
                stream,
                text,
            });
        }
    });
}

fn kill_pid(pid: u32) {
    #[cfg(unix)]
    unsafe {
        let _ = kill(pid as i32, 9);
    }
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
    }
}

#[cfg(unix)]
unsafe extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
}

async fn pump_lines<R: tokio::io::AsyncRead + Unpin>(
    reader: R,
    stream: LogStream,
    id: InstanceId,
    events: &tokio::sync::broadcast::Sender<Event>,
    tokens: &[String],
) {
    let mut lines = BufReader::new(reader).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let text = redact_line_with_tokens(&line, tokens);
        let _ = events.send(Event::LogLine {
            instance_id: id,
            stream,
            text,
        });
    }
}

fn record_session(db: &Path, id: InstanceId, elapsed: Duration) {
    let Ok(store) = Store::open_file(db) else {
        return;
    };
    let Ok(Some(mut row)) = store.get_instance(id) else {
        return;
    };
    row.playtime_secs = row.playtime_secs.saturating_add(elapsed.as_secs() as i64);
    row.session_count = row.session_count.saturating_add(1);
    row.last_played_at = Some(now_ms());
    let _ = store.update_instance(&row);
}

#[derive(Deserialize)]
struct VersionManifest {
    versions: Vec<ManifestVersion>,
}

#[derive(Deserialize)]
struct ManifestVersion {
    id: String,
    url: String,
    sha1: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::VERSION_MANIFEST_URL;
    use crate::error::EngineError;
    use crate::ids::Loader;
    use crate::mojang::FeatureSet;
    use crate::store::MemoryKeychain;
    use crate::types::{CreateInstance, ProgressSink, QuickPlay};
    use crate::{Engine, LauncherPaths};
    use tokio_util::sync::CancellationToken;

    struct NoopProgress;

    impl ProgressSink for NoopProgress {
        fn set(&self, _title: &str, _done: u64, _total: u64) {}
    }

    async fn test_engine() -> Engine {
        let root = tempfile::tempdir().unwrap();
        let paths = LauncherPaths::new(root.path().to_path_buf());
        paths.create_dirs().unwrap();
        let kc = MemoryKeychain::new();
        let engine = Engine::open_with_keychain(paths, &kc).unwrap();
        std::mem::forget(root);
        engine
    }

    #[test]
    fn prepare_world_sets_singleplayer_feature() {
        let (feat, single, multi) = super::quick_play_launch(Some(QuickPlay::World {
            folder: "New World".into(),
        }));
        assert_eq!(
            feat,
            FeatureSet {
                quick_play_single: true,
                ..FeatureSet::default()
            }
        );
        assert_eq!(single.as_deref(), Some("New World"));
        assert_eq!(multi, None);
    }

    #[test]
    fn prepare_server_sets_multiplayer_feature() {
        let (feat, single, multi) = super::quick_play_launch(Some(QuickPlay::Server {
            address: "mc.hypixel.net".into(),
        }));
        assert_eq!(
            feat,
            FeatureSet {
                quick_play_multi: true,
                ..FeatureSet::default()
            }
        );
        assert_eq!(multi.as_deref(), Some("mc.hypixel.net"));
        assert_eq!(single, None);
    }

    #[test]
    fn version_manifest_url_is_piston_meta() {
        assert_eq!(
            VERSION_MANIFEST_URL,
            "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json"
        );
    }

    #[tokio::test]
    async fn prepare_vanilla_offline_errors_without_account() {
        let engine = test_engine().await;
        let id = engine
            .create_instance(CreateInstance {
                name: "V".into(),
                minecraft_version: "1.21.1".into(),
                loader: Loader::Vanilla,
                loader_version: None,
                icon_png: None,
            })
            .await
            .unwrap();
        let err = engine
            .prepare(id, &NoopProgress, CancellationToken::new(), None)
            .await
            .unwrap_err();
        assert!(matches!(err, EngineError::NoAccount));
    }

    #[tokio::test]
    async fn prepare_fabric_offline_errors_without_account() {
        let engine = test_engine().await;
        let id = engine
            .create_instance(CreateInstance {
                name: "F".into(),
                minecraft_version: "1.21.1".into(),
                loader: Loader::Fabric,
                loader_version: None,
                icon_png: None,
            })
            .await
            .unwrap();
        let err = engine
            .prepare(id, &NoopProgress, CancellationToken::new(), None)
            .await
            .unwrap_err();
        assert!(
            !matches!(
                err,
                EngineError::LoaderUnavailable {
                    loader: Loader::Fabric,
                    ..
                }
            ),
            "fabric prepare must proceed past LoaderUnavailable, got {err:?}"
        );
        assert!(matches!(err, EngineError::NoAccount));
    }

    #[tokio::test]
    async fn prepare_forge_offline_errors_without_account() {
        let engine = test_engine().await;
        let id = engine
            .create_instance(CreateInstance {
                name: "G".into(),
                minecraft_version: "1.21.1".into(),
                loader: Loader::Forge,
                loader_version: None,
                icon_png: None,
            })
            .await
            .unwrap();
        let err = engine
            .prepare(id, &NoopProgress, CancellationToken::new(), None)
            .await
            .unwrap_err();
        assert!(
            !matches!(
                err,
                EngineError::LoaderUnavailable {
                    loader: Loader::Forge,
                    ..
                }
            ),
            "forge prepare must proceed past LoaderUnavailable, got {err:?}"
        );
        assert!(matches!(err, EngineError::NoAccount));
    }
}
