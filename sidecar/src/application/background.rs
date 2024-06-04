use either::Either;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::RwLock;
use std::thread;
use std::{future::Future, pin::Pin};
use thread_priority::ThreadBuilderExt;
use tokio::sync::OwnedSemaphorePermit;
use tokio::sync::Semaphore;
use tracing::debug;
use tracing::error;
use tracing::info;

use crate::repo::state::RepoError;
use crate::repo::types::RepoMetadata;
use crate::repo::types::Repository;
use crate::repo::types::{RepoRef, SyncStatus};
use crate::webserver::repos::QueueState;
use crate::webserver::repos::QueuedRepoStatus;

use super::application::Application;
use super::config::configuration::Configuration;

#[derive(serde::Serialize, Clone, Debug)]
pub struct Progress {
    #[serde(rename = "ref")]
    pub reporef: RepoRef,
    #[serde(rename = "ev")]
    pub event: ProgressEvent,
}

#[derive(serde::Serialize, Clone, Debug)]
#[serde(rename_all = "snake_case")]
pub enum ProgressEvent {
    IndexPercent(u8),
    StatusChange(SyncStatus),
}

// This is just a tokio sender which sends progress events for a given repo and
type ProgressStream = tokio::sync::broadcast::Sender<Progress>;

#[derive(Clone)]
pub struct ProgressStreamWithContext {
    sender: ProgressStream,
    context: String,
}

#[derive(Clone)]
pub struct SyncQueue {
    runner: BackgroundExecutor,
    active: Arc<scc::HashMap<RepoRef, Arc<SyncHandle>>>,
    tickets: Arc<Semaphore>,
    pub(crate) queue: Arc<NotifyQueue>,

    /// Report progress from indexing runs
    pub(crate) progress: ProgressStreamWithContext,
}

impl SyncQueue {
    pub fn start(config: Arc<Configuration>) -> Self {
        let (progress, _) = tokio::sync::broadcast::channel(config.max_threads * 2);

        let rand_id: u64 = rand::random();

        let progress_stream_with_context = ProgressStreamWithContext {
            sender: progress,
            context: format!("sync-queue-{}", rand_id),
        };

        let instance = Self {
            tickets: Arc::new(Semaphore::new(config.max_threads)),
            runner: BackgroundExecutor::start(config.clone()),
            active: Default::default(),
            queue: Default::default(),
            progress: progress_stream_with_context,
        };

        {
            let instance = instance.clone();

            // We spawn the queue handler on the background executor
            instance.runner.clone().spawn(async move {
                while let (Ok(permit), next) = tokio::join!(
                    instance.tickets.clone().acquire_owned(),
                    instance
                        .queue
                        .pop_if(|h| !instance.active.contains(&h.reporef))
                ) {
                    let active = Arc::clone(&instance.active);
                    match active
                        .insert_async(next.reporef.clone(), next.clone())
                        .await
                    {
                        Ok(_) => {
                            // tokio::task::spawn(async move {
                            //     info!(?next.reporef, "starting indexing of repository");

                            //     let result = next.run(permit).await;
                            //     _ = active.remove(&next.reporef);

                            //     debug!(?result, "indexing finished for repository");
                            // });
                        }
                        Err((_, next)) => {
                            // this shouldn't happen, but we can handle it gracefully
                            instance.queue.push(next).await
                        }
                    };
                }
            });
        }

        instance
    }

    pub fn bind(&self, app: Application) -> BoundSyncQueue {
        BoundSyncQueue(app, self.clone())
    }

    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<Progress> {
        self.progress.sender.subscribe()
    }

    pub fn get_progress_context(&self) -> &str {
        &self.progress.context
    }

    pub async fn read_queue(&self) -> Vec<QueuedRepoStatus> {
        let mut output = vec![];
        self.active
            .scan_async(|_, handle| {
                output.push(QueuedRepoStatus {
                    reporef: handle.reporef.clone(),
                    state: QueueState::Active,
                });
            })
            .await;

        for handle in self.queue.get_list().await {
            output.push(QueuedRepoStatus {
                reporef: handle.reporef.clone(),
                state: QueueState::Queued,
            });
        }

        output
    }
}

/// Asynchronous queue with await semantics for popping the front
/// element.
pub(crate) struct NotifyQueue {
    queue: tokio::sync::RwLock<VecDeque<Arc<SyncHandle>>>,
    available: Semaphore,
}

impl Default for NotifyQueue {
    fn default() -> Self {
        Self {
            queue: Default::default(),
            available: Semaphore::new(0),
        }
    }
}

impl NotifyQueue {
    pub(crate) async fn push(&self, item: Arc<SyncHandle>) {
        let mut q = self.queue.write().await;

        self.available.add_permits(1);

        q.push_back(item);
    }

    pub(super) async fn pop_if(&self, pred: impl Fn(&SyncHandle) -> bool) -> Arc<SyncHandle> {
        loop {
            let permit = self.available.acquire().await.expect("fatal");
            let mut q = self.queue.write().await;

            let first = q.iter().position(|h| (pred)(h));

            if let Some(pos) = first {
                permit.forget();
                return q.remove(pos).expect("locked");
            }
        }
    }

    #[allow(unused)]
    pub(super) async fn get_list(&self) -> Vec<Arc<SyncHandle>> {
        self.queue.read().await.iter().cloned().collect()
    }

    pub(super) async fn contains(&self, reporef: &RepoRef) -> bool {
        self.queue
            .read()
            .await
            .iter()
            .any(|h| &h.reporef == reporef)
    }

    #[allow(unused)]
    pub(super) async fn remove(&self, reporef: RepoRef) {
        let mut q = self.queue.write().await;
        self.available.acquire().await.expect("fatal").forget();
        q.retain(|item| item.reporef != reporef);
    }
}

type Task = Pin<Box<dyn Future<Output = ()> + Send + Sync>>;

/// This has a sender where we can send future's to be executed in the background
/// and get back a future which we can await on
#[derive(Clone)]
pub struct BackgroundExecutor {
    sender: flume::Sender<Task>,
}

// This is bound the to the application and the sync queue which will run things
// in the background.
pub struct BoundSyncQueue(pub(crate) Application, pub(crate) SyncQueue);

impl BoundSyncQueue {
    /// Enqueue repos for syncing with the current configuration.
    ///
    /// Skips any repositories in the list which are already queued or being synced.
    /// Returns the number of new repositories queued for syncing.
    pub async fn enqueue_sync(self, repositories: Vec<RepoRef>) -> usize {
        let mut num_queued = 0;

        for reporef in repositories {
            if self.1.queue.contains(&reporef).await || self.1.active.contains(&reporef) {
                continue;
            }

            info!(?reporef, "queueing for sync");
            let handle = SyncHandle::new(self.0.clone(), reporef, self.1.progress.clone()).await;
            self.1.queue.push(handle).await;
            num_queued += 1;
        }

        num_queued
    }

    /// Block until the repository sync & index process is complete.
    ///
    /// Returns the new status.
    pub(crate) async fn block_until_synced(self, reporef: RepoRef) -> anyhow::Result<SyncStatus> {
        let handle = SyncHandle::new(self.0.clone(), reporef, self.1.progress.clone()).await;
        let finished = handle.notify_done();
        self.1.queue.push(handle).await;
        Ok(finished.recv_async().await?)
    }

    pub async fn startup_scan(self) -> anyhow::Result<()> {
        let Self(Application { repo_pool, .. }, _) = &self;

        let mut repos = vec![];
        repo_pool.scan_async(|k, _| repos.push(k.clone())).await;

        self.enqueue_sync(repos).await;

        Ok(())
    }
}

impl BackgroundExecutor {
    fn start(config: Arc<Configuration>) -> Self {
        let (sender, receiver) = flume::unbounded();

        let tokio: Arc<_> = tokio::runtime::Builder::new_multi_thread()
            .thread_name("codestory-bg-threads")
            .worker_threads(config.max_threads)
            .max_blocking_threads(config.max_threads)
            .enable_time()
            .enable_io()
            .build()
            .unwrap()
            .into();

        let tokio_ref = tokio.clone();
        // test can re-initialize the app, and we shouldn't fail
        _ = rayon::ThreadPoolBuilder::new()
            .spawn_handler(move |thread| {
                let tokio_ref = tokio_ref.clone();

                let thread_priority = thread_priority::ThreadPriority::Max;

                std::thread::Builder::new()
                    .name("index-worker".to_owned())
                    .spawn_with_priority(thread_priority, move |_| {
                        let _tokio = tokio_ref.enter();
                        thread.run()
                    })
                    .map(|_| ())
            })
            .num_threads(config.max_threads)
            .build_global();

        thread::spawn(move || {
            while let Ok(task) = receiver.recv() {
                tokio.spawn(task);
            }
        });

        Self { sender }
    }

    fn spawn<T>(&self, job: impl Future<Output = T> + Send + Sync + 'static) {
        self.sender
            .send(Box::pin(async move {
                job.await;
            }))
            .unwrap();
    }

    #[allow(unused)]
    pub async fn wait_for<T: Send + Sync + 'static>(
        &self,
        job: impl Future<Output = T> + Send + Sync + 'static,
    ) -> T {
        let (s, r) = flume::bounded(1);
        self.spawn(async move { s.send_async(job.await).await.unwrap() });
        r.recv_async().await.unwrap()
    }
}

enum ControlEvent {
    /// Cancel whatever's happening, and return
    Cancel,
    /// Remove, is when the user has asked us to remove it for whatever reason
    Remove,
}

pub struct SyncPipes {
    reporef: RepoRef,
    progress: ProgressStreamWithContext,
    event: RwLock<Option<ControlEvent>>,
}

impl SyncPipes {
    pub(super) fn new(reporef: RepoRef, progress: ProgressStreamWithContext) -> Self {
        Self {
            reporef,
            progress,
            event: Default::default(),
        }
    }

    pub fn index_percent(&self, current: u8) {
        let context = &self.progress.context;
        debug!(?current, ?context, "index percent");
        _ = self.progress.sender.send(Progress {
            reporef: self.reporef.clone(),
            event: ProgressEvent::IndexPercent(current),
        });
    }

    pub(crate) fn status(&self, new: SyncStatus) {
        _ = self.progress.sender.send(Progress {
            reporef: self.reporef.clone(),
            event: ProgressEvent::StatusChange(new),
        });
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        matches!(
            self.event.read().unwrap().as_ref(),
            Some(ControlEvent::Cancel)
        )
    }

    pub(crate) fn is_removed(&self) -> bool {
        matches!(
            self.event.read().unwrap().as_ref(),
            Some(ControlEvent::Remove)
        )
    }
}

pub struct SyncHandle {
    pub reporef: RepoRef,
    pub pipes: SyncPipes,
    app: Application,
    exited: flume::Sender<SyncStatus>,
    exit_signal: flume::Receiver<SyncStatus>,
}

type Result<T> = std::result::Result<T, SyncError>;
#[derive(thiserror::Error, Debug)]
pub(super) enum SyncError {
    #[error("tantivy: {0:?}")]
    Tantivy(anyhow::Error),

    #[error("cancelled by user")]
    Cancelled,

    #[error("indexing failed: {0:?}")]
    Indexing(RepoError),
}

impl PartialEq for SyncHandle {
    fn eq(&self, other: &Self) -> bool {
        self.reporef == other.reporef
    }
}

impl Drop for SyncHandle {
    fn drop(&mut self) {
        let status = self.set_status(|v| {
            use SyncStatus::*;
            match &v.sync_status {
                Indexing | Syncing => Error {
                    message: "unknown".into(),
                },
                Cancelling => Cancelled,
                other => other.clone(),
            }
        });

        _ = self
            .app
            .config
            .state_source
            .save_pool(self.app.repo_pool.clone());

        info!(?status, ?self.reporef, "normalized status after sync");
        if self
            .exited
            .send(status.unwrap_or(SyncStatus::Removed))
            .is_err()
        {
            debug!("file pointer no longer exists");
        }
    }
}

/// This is the handler for syncing and make sure that we can lock things properly
/// and not run it over the budget
impl SyncHandle {
    pub async fn new(
        app: Application,
        reporef: RepoRef,
        status: ProgressStreamWithContext,
    ) -> Arc<Self> {
        let (exited, exit_signal) = flume::bounded(1);
        let pipes = SyncPipes::new(reporef.clone(), status);
        let current = app
            .repo_pool
            .entry_async(reporef.clone())
            .await
            .or_insert_with(|| Repository::local_from(&reporef));

        let sh = Self {
            app: app.clone(),
            reporef: reporef.clone(),
            pipes,
            exited,
            exit_signal,
        };

        sh.pipes.status(current.get().sync_status.clone());
        sh.into()
    }

    pub fn notify_done(&self) -> flume::Receiver<SyncStatus> {
        self.exit_signal.clone()
    }

    /// The permit that's taken here is exclusively for parallelism control.
    pub(super) async fn run(&self, _permit: OwnedSemaphorePermit) -> Result<SyncStatus> {
        debug!(?self.reporef, "indexing repository");
        let Application { ref repo_pool, .. } = self.app;

        // skip git operations if the repo has been marked as removed
        // if the ref is non-existent, sync it and add it to the pool
        let removed = repo_pool
            .read_async(&self.reporef, |_k, v| v.sync_status == SyncStatus::Removed)
            .await
            .unwrap_or(false);

        if !removed {
            match self.git_sync().await {
                Ok(status) => {
                    if let SyncStatus::Done = self.set_status(|_| status).unwrap() {
                        return Ok(SyncStatus::Done);
                    }
                }
                Err(err) => {
                    error!(?err, ?self.reporef, "failed to sync repository");
                    self.set_status(|_| SyncStatus::Error {
                        message: err.to_string(),
                    })
                    .unwrap();
                    return Err(err);
                }
            }
        }

        if self.pipes.is_cancelled() && !self.pipes.is_removed() {
            self.set_status(|_| SyncStatus::Cancelled);
            debug!(?self.reporef, "cancelled while cloning");
            return Err(SyncError::Cancelled);
        }

        let repository = repo_pool
            .read_async(&self.reporef, |_k, v| v.clone())
            .await
            .unwrap();

        let git_statistics = if repository.last_index_unix_secs == 0 {
            let db = self.app.sql.clone();
            let reporef = self.reporef.clone();

            Some(tokio::task::spawn(
                crate::git::commit_statistics::git_commit_statistics(reporef.clone(), db),
            ))
        } else {
            None
        };

        let indexed = self.index().await;
        let status = match indexed {
            Ok(Either::Left(status)) => Some(status),
            Ok(Either::Right(state)) => {
                self.app
                    .repo_pool
                    .update(&self.reporef, |_k, repo| repo.sync_done_with(state));

                // Log here if getting git_statistics failed
                if let Some(git_statistics) = git_statistics {
                    if let Err(err) = git_statistics.await {
                        error!(?err, "failed to get git statistics");
                    }
                }

                // technically `sync_done_with` does this, but we want to send notifications
                self.set_status(|_| SyncStatus::Done)
            }
            Err(SyncError::Cancelled) => self.set_status(|_| SyncStatus::Cancelled),
            Err(err) => {
                error!(?err, ?self.reporef, "failed to index repository");
                self.set_status(|_| SyncStatus::Error {
                    message: err.to_string(),
                })
            }
        };

        Ok(status.expect("failed to update repo status"))
    }

    async fn index(&self) -> Result<Either<SyncStatus, Arc<RepoMetadata>>> {
        use SyncStatus::*;
        let Application {
            // ref indexes,
            ref repo_pool,
            ..
        } = self.app;

        // let writers = indexes.writers().await.map_err(SyncError::Tantivy)?;
        let repo = {
            let orig = repo_pool
                .read_async(&self.reporef, |_k, v| v.clone())
                .await
                .unwrap();
            orig
        };

        let instant = std::time::Instant::now();

        let indexed = match repo.sync_status {
            current @ (Uninitialized | Syncing | Indexing) => return Ok(Either::Left(current)),
            Removed => return Ok(either::Left(SyncStatus::Removed)),
            RemoteRemoved => {
                // Note we don't clean up here, leave the
                // bare bones behind.
                //
                // This is to be able to report to the user that
                // something happened, and let them clean up in a
                // subsequent action.
                return Ok(Either::Left(RemoteRemoved));
            }
            _ => {
                self.set_status(|_| Indexing).unwrap();
                // writers.index(self, &repo).await.map(Either::Right)
            }
        };

        let time_taken = instant.elapsed();

        debug!(?self.reporef, ?time_taken, "indexing finished");

        Err(SyncError::Cancelled)
        // match indexed {
        //     Ok(_) => {
        //         debug!("committing index");
        //         // writers.commit().map_err(SyncError::Tantivy)?;
        //         debug!("finished committing index");
        //         // indexed.map_err(SyncError::Indexing)
        //         Err(SyncError::Cancelled)
        //     }
        //     // Err(_) if self.pipes.is_removed() => self.delete_repo(&repo, writers).await,
        //     Err(_) if self.pipes.is_cancelled() => {
        //         // writers.rollback().map_err(SyncError::Tantivy)?;
        //         debug!(?self.reporef, "index cancelled by user");
        //         Err(SyncError::Cancelled)
        //     }
        //     Err(err) => {
        //         // writers.rollback().map_err(SyncError::Tantivy)?;
        //         Err(SyncError::Indexing(err))
        //     }
        // }
    }

    // async fn delete_repo(
    //     &self,
    //     repo: &Repository,
    //     writers: indexes::GlobalWriteHandle<'_>,
    // ) -> Result<Either<SyncStatus, Arc<RepoMetadata>>> {
    //     self.app.repo_pool.remove(&self.reporef);

    //     let deleted = self.delete_repo_indexes(repo, &writers).await;
    //     if deleted.is_ok() {
    //         writers.commit().await.map_err(SyncError::Tantivy)?;
    //         self.app
    //             .config
    //             .source
    //             .save_pool(self.app.repo_pool.clone())
    //             .map_err(SyncError::State)?;
    //     }

    //     deleted.map(|_| Either::Left(SyncStatus::Removed))
    // }

    async fn git_sync(&self) -> Result<SyncStatus> {
        // Since we always assume local is correct, we don't have to sync
        // or test for things yet...
        // TODO(codestory): Make this more resilient in the future and add
        // support for making it work with remote repos
        Ok(SyncStatus::Queued)
    }
    // let repo = self.reporef.clone();
    // let backend = repo.backend();
    // let creds = match self.app.credentials.for_repo(&repo) {
    //     Some(creds) => creds,
    //     None => {
    //         let Some(path) = repo.local_path() else {
    //             return Err(SyncError::NoKeysForBackend(backend));
    //         };

    //         if !self.app.allow_path(&path) {
    //             return Err(SyncError::PathNotAllowed(path));
    //         }

    //         // we _never_ touch the git repositories of local repos
    //         return Ok(SyncStatus::Queued);
    //     }
    // };

    //     let repo = self.sync_lock().await?;

    //     // This reads really badly, but essentially we need a way to
    //     // retry after cleaning things up, and duplicating _too much_
    //     // code.
    //     let mut loop_counter = 0;
    //     let loop_max = 1;
    //     let git_err = loop {
    //         match creds.git_sync(&self.reporef, repo.clone()).await {
    //             Err(
    //                 err @ RemoteError::GitCloneFetch(gix::clone::fetch::Error::PrepareFetch(
    //                     gix::remote::fetch::prepare::Error::RefMap(
    //                         gix::remote::ref_map::Error::Handshake(
    //                             gix::protocol::handshake::Error::InvalidCredentials { .. },
    //                         ),
    //                     ),
    //                 )),
    //             ) => {
    //                 error!(?err, ?self.reporef, "invalid credentials for accessing git repo");
    //                 return Err(SyncError::Sync(err));
    //             }
    //             Err(
    //                 err @ RemoteError::GitOpen(_)
    //                 | err @ RemoteError::GitFetch(_)
    //                 | err @ RemoteError::GitPrepareFetch(_)
    //                 | err @ RemoteError::GitClone(_)
    //                 | err @ RemoteError::GitCloneFetch(_)
    //                 | err @ RemoteError::GitConnect(_)
    //                 | err @ RemoteError::GitFindRemote(_),
    //             ) => {
    //                 _ = tokio::fs::remove_dir_all(&repo.disk_path).await;

    //                 if loop_counter == loop_max {
    //                     break err;
    //                 }

    //                 loop_counter += 1;
    //             }
    //             Err(RemoteError::RemoteNotFound) => {
    //                 error!(?repo, "remote repository removed; disabling local syncing");

    //                 // we want indexing to pick this up later and handle the new state
    //                 // all local cleanups are done, so everything should be consistent
    //                 return Ok(SyncStatus::RemoteRemoved);
    //             }
    //             Err(RemoteError::GitHub(
    //                 octocrab::Error::Service { .. }
    //                 | octocrab::Error::Hyper { .. }
    //                 | octocrab::Error::Http { .. },
    //             )) => {
    //                 warn!("likely network error, skipping further syncing");
    //                 return Ok(SyncStatus::Done);
    //             }
    //             Err(err) => {
    //                 error!(?err, ?self.reporef, "failed to sync repository");
    //                 return Err(SyncError::Sync(err));
    //             }
    //             Ok(status) => {
    //                 self.app
    //                     .config
    //                     .source
    //                     .save_pool(self.app.repo_pool.clone())
    //                     .expect("filesystem error");

    //                 return Ok(status);
    //             }
    //         }
    //     };

    //     Err(SyncError::Sync(git_err))
    // }

    // async fn delete_repo_indexes(
    //     &self,
    //     repo: &Repository,
    //     writers: &indexes::GlobalWriteHandleRef<'_>,
    // ) -> Result<()> {
    //     let Application {
    //         ref semantic,
    //         ref sql,
    //         ..
    //     } = self.app;

    //     if let Some(semantic) = semantic {
    //         semantic
    //             .delete_points_for_hash(&self.reporef.to_string(), std::iter::empty())
    //             .await;
    //     }

    //     FileCache::for_repo(sql, semantic.as_ref(), &self.reporef)
    //         .delete()
    //         .await
    //         .map_err(SyncError::Sql)?;

    //     if !self.reporef.is_local() {
    //         tokio::fs::remove_dir_all(&repo.disk_path)
    //             .await
    //             .map_err(|e| SyncError::RemoveLocal(repo.disk_path.clone(), e))?;
    //     }

    //     for handle in writers {
    //         handle.delete(repo);
    //     }

    //     Ok(())
    // }

    pub fn pipes(&self) -> &SyncPipes {
        &self.pipes
    }

    pub(crate) fn set_status(
        &self,
        updater: impl FnOnce(&Repository) -> SyncStatus,
    ) -> Option<SyncStatus> {
        let new_status = self.app.repo_pool.update(&self.reporef, move |_k, repo| {
            repo.sync_status = (updater)(repo);
            repo.sync_status.clone()
        })?;

        debug!(?self.reporef, ?new_status, "new status");
        self.pipes.status(new_status.clone());
        Some(new_status)
    }
}
