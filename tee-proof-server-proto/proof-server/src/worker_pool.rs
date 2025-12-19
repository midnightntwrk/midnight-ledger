// This file is part of midnight-ledger.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0
// Licensed under the Apache License, Version 2.0 (the "License");
// You may not use this file except in compliance with the License.
// You may obtain a copy of the License at
// http://www.apache.org/licenses/LICENSE-2.0
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use async_channel::{Receiver, RecvError, Sender, bounded, unbounded};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use thiserror::Error;
use tokio::{runtime::Handle, sync::Mutex, task};
use uuid::Uuid;

#[derive(Error, Debug)]
pub enum WorkerPoolError {
    #[error("Job Queue full")]
    JobQueueFull,
    #[error("Work channel closed")]
    ChannelClosed,
    #[error("Job Missing")]
    JobMissing(Uuid),
    #[error("Tried to cancel job, but job wasn't pending")]
    JobNotPending(Uuid),
}

#[derive(Error, Clone, Debug, PartialEq, Eq)]
pub enum WorkError {
    #[error("bad input")]
    BadInput(String),
    #[error("internal error")]
    InternalError(String),
    #[error("work cancelled unexpectedly")]
    CancelledUnexpectedly,
    #[error("task join error")]
    JoinError,
}

#[derive(Clone)]
pub enum JobStatus {
    Pending,
    Processing,
    Cancelled,
    Error(WorkError),
    Success(Vec<u8>),
}

impl JobStatus {
    pub async fn wait_for_success(updates: &Receiver<JobStatus>) -> Result<Vec<u8>, WorkError> {
        while let Ok(status) = updates.recv().await {
            match status {
                JobStatus::Success(res) => return Ok(res),
                JobStatus::Error(e) => return Err(e),
                JobStatus::Cancelled => return Err(WorkError::CancelledUnexpectedly),
                _ => continue,
            }
        }
        Err(WorkError::InternalError(
            "failed to complete job".to_string(),
        ))
    }
}

pub struct WorkItem {
    id: Uuid,
    work: Box<dyn FnOnce(Handle) -> Result<Vec<u8>, WorkError> + Send + 'static>,
}

#[derive(Clone)]
pub struct Job {
    status: JobStatus,
    update_sender: Sender<JobStatus>,
    update_receiver: Arc<Receiver<JobStatus>>,
    timeout: SystemTime,
}

impl Job {
    fn new(ttl: Duration) -> Self {
        let (update_sender, receiver) = unbounded();
        Self {
            update_sender,
            update_receiver: Arc::new(receiver),
            status: JobStatus::Pending,
            timeout: SystemTime::now() + ttl,
        }
    }

    fn updates(&self) -> Arc<Receiver<JobStatus>> {
        self.update_receiver.clone()
    }
}

#[derive(Clone)]
pub struct Requests {
    pub capacity: usize,
    job_ttl: Duration,
    reqs: Arc<Mutex<HashMap<Uuid, Job>>>,
}

impl Requests {
    fn new(capacity: usize, job_ttl: Duration) -> Self {
        Self {
            capacity,
            job_ttl,
            reqs: Default::default(),
        }
    }

    pub async fn processing_count(&self) -> usize {
        self.reqs
            .lock()
            .await
            .values()
            .filter(|j| matches!(j.status, JobStatus::Processing))
            .count()
    }

    pub async fn pending_count(&self) -> usize {
        self.reqs
            .lock()
            .await
            .values()
            .filter(|j| matches!(j.status, JobStatus::Pending))
            .count()
    }

    pub async fn is_full(&self) -> bool {
        self.capacity != 0 && self.pending_count().await >= self.capacity
    }

    async fn set_status(&self, id: &Uuid, status: JobStatus) {
        if let Some(req) = self.reqs.lock().await.get_mut(id) {
            req.status = status.clone();
            req.update_sender.send(status).await.unwrap();
        }
    }

    async fn new_req(&self) -> Result<Uuid, WorkerPoolError> {
        if self.is_full().await {
            return Err(WorkerPoolError::JobQueueFull);
        }
        let req = Job::new(self.job_ttl);
        let id = Uuid::new_v4();
        self.reqs.lock().await.insert(id, req);
        Ok(id)
    }

    async fn get(&self, id: Uuid) -> Option<Job> {
        self.reqs.lock().await.get(&id).cloned()
    }

    async fn gc(&self) {
        let mut reqs = self.reqs.lock().await;
        let mut keys_to_remove = vec![];
        let now = SystemTime::now();
        for (id, req) in reqs.iter() {
            if req.timeout < now {
                keys_to_remove.push(*id);
            }
        }
        for key in keys_to_remove {
            reqs.remove(&key);
        }
    }
}

/// Worker pool that processes work items in parallel
pub struct WorkerPool {
    sender: Sender<WorkItem>,
    workers: Vec<task::JoinHandle<()>>,
    pub requests: Requests,
    job_gc_handle: task::JoinHandle<()>,
    gc_stop_sender: Sender<()>,
}

impl WorkerPool {
    /// Creates a new worker pool with the specified number of workers and channel capacity
    pub fn new(num_workers: usize, job_capacity: usize, job_timeout_secs: f64) -> Self {
        let (work_sender, work_receiver) = unbounded::<WorkItem>();
        let work_receiver = Arc::new(work_receiver);
        let requests = Requests::new(job_capacity, Duration::from_secs_f64(job_timeout_secs));
        let mut workers = Vec::with_capacity(num_workers);

        // Get the current runtime handle to share across all workers
        let runtime_handle = Handle::current();

        for _ in 0..num_workers {
            let work_receiver = Arc::clone(&work_receiver);
            let requests = requests.clone();
            let runtime_handle = runtime_handle.clone();

            let handle = task::spawn(async move {
                while let Ok(work) = work_receiver.recv().await {
                    let status = requests.get(work.id).await;
                    if status.is_none()
                        || matches!(
                            status,
                            Some(Job {
                                status: JobStatus::Cancelled,
                                ..
                            })
                        )
                    {
                        // Ignore work
                        continue;
                    }
                    requests.set_status(&work.id, JobStatus::Processing).await;

                    // Pass the shared runtime handle to the work closure
                    let runtime_handle_for_work = runtime_handle.clone();
                    let result = task::spawn_blocking(move || (work.work)(runtime_handle_for_work)).await;

                    match result {
                        Ok(Ok(res)) => requests.set_status(&work.id, JobStatus::Success(res)).await,
                        Ok(Err(e)) => requests.set_status(&work.id, JobStatus::Error(e)).await,
                        Err(_e) => {
                            requests
                                .set_status(&work.id, JobStatus::Error(WorkError::JoinError))
                                .await
                        }
                    }
                }
            });

            workers.push(handle);
        }

        let (gc_stop_sender, gc_stop_receiver) = bounded::<()>(1);
        let job_gc_handle = {
            let requests = requests.clone();
            tokio::spawn(async move {
                loop {
                    match tokio::time::timeout(Duration::from_secs(10), gc_stop_receiver.recv())
                        .await
                    {
                        Ok(Ok(_)) => break,
                        Ok(Err(RecvError)) => break,
                        // Timeout case
                        Err(_) => requests.gc().await,
                    }
                }
            })
        };

        WorkerPool {
            sender: work_sender,
            workers,
            requests,
            job_gc_handle,
            gc_stop_sender,
        }
    }

    /// Submits a work item to the pool
    pub async fn submit<F>(&self, work: F) -> Result<Uuid, WorkerPoolError>
    where
        F: FnOnce(Handle) -> Result<Vec<u8>, WorkError> + Send + 'static,
    {
        let id = self.requests.new_req().await?;
        self.sender
            .send(WorkItem {
                id,
                work: Box::new(work),
            })
            .await
            .map_err(|_| WorkerPoolError::ChannelClosed)?;
        Ok(id)
    }

    /// Submits a work item to the pool and returns both the job ID and a subscription to its status updates
    /// Returns None if the submission fails
    pub async fn submit_and_subscribe<F>(
        &self,
        work: F,
    ) -> Result<(Uuid, Arc<Receiver<JobStatus>>), WorkerPoolError>
    where
        F: FnOnce(Handle) -> Result<Vec<u8>, WorkError> + Send + 'static,
    {
        let id = self.submit(work).await?;
        Ok((id, self.subscribe(id).await.unwrap()))
    }

    /// Receives the result of a completed work item
    pub async fn poll(&self, id: Uuid) -> Option<JobStatus> {
        let job = self.requests.get(id).await;
        if let Some(ref job) = job {
            if job.timeout < SystemTime::now() {
                return None;
            }
            Some(job.status.clone())
        } else {
            None
        }
    }

    /// Cancels a job with the given ID
    /// Returns the job if it was found and canceled, None otherwise
    pub async fn cancel(&self, id: Uuid) -> Result<(), WorkerPoolError> {
        match self.requests.get(id).await {
            Some(job) => {
                if matches!(job.status, JobStatus::Pending) {
                    self.requests.set_status(&id, JobStatus::Cancelled).await;
                    Ok(())
                } else {
                    Err(WorkerPoolError::JobNotPending(id))
                }
            }
            _ => Err(WorkerPoolError::JobMissing(id)),
        }
    }

    /// Subscribes to status updates for a specific job
    /// Returns a receiver that will receive all status updates for the job
    /// Returns None if the job doesn't exist or has already timed out
    pub async fn subscribe(&self, id: Uuid) -> Result<Arc<Receiver<JobStatus>>, WorkerPoolError> {
        let job = self
            .requests
            .get(id)
            .await
            .ok_or(WorkerPoolError::JobMissing(id))?;
        if job.timeout < SystemTime::now() {
            return Err(WorkerPoolError::JobMissing(id));
        }
        Ok(job.updates())
    }

    /// Shuts down the worker pool, waiting for all workers to complete
    pub async fn shutdown(self) {
        drop(self.sender);
        drop(self.gc_stop_sender);
        for worker in self.workers {
            worker.await.unwrap();
        }
        self.job_gc_handle.await.unwrap();
    }
}
