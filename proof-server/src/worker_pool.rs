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

use actix_web::error::{
    ErrorBadRequest, ErrorInternalServerError, ErrorPreconditionRequired, ErrorTooManyRequests,
};
use async_channel::{Receiver, RecvError, Sender, bounded, unbounded};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use thiserror::Error;
use tokio::{sync::Mutex, task};
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

impl From<WorkError> for actix_web::Error {
    fn from(val: WorkError) -> Self {
        match val {
            e @ WorkError::BadInput(_) => ErrorBadRequest(e),
            e @ WorkError::InternalError(_) => ErrorInternalServerError(e),
            e @ WorkError::CancelledUnexpectedly => ErrorInternalServerError(e),
            e @ WorkError::JoinError => ErrorInternalServerError(e),
        }
    }
}

impl From<WorkerPoolError> for actix_web::Error {
    fn from(val: WorkerPoolError) -> Self {
        match val {
            e @ WorkerPoolError::JobQueueFull => ErrorTooManyRequests(e),
            e @ WorkerPoolError::ChannelClosed => ErrorInternalServerError(e),
            e @ WorkerPoolError::JobMissing(_uuid) => ErrorPreconditionRequired(e),
            e @ WorkerPoolError::JobNotPending(_uuid) => ErrorBadRequest(e),
        }
    }
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
    work: Box<dyn FnOnce() -> Result<Vec<u8>, WorkError> + Send + 'static>,
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

        for _ in 0..num_workers {
            let work_receiver = Arc::clone(&work_receiver);
            let requests = requests.clone();

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
                    let result = task::spawn_blocking(work.work).await;
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
        F: FnOnce() -> Result<Vec<u8>, WorkError> + Send + 'static,
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
        F: FnOnce() -> Result<Vec<u8>, WorkError> + Send + 'static,
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn test_worker_pool_basic() {
        let pool = WorkerPool::new(4, 10, 2.0);

        // Submit some work using closures
        let mut ids = vec![];
        for i in 0..5u32 {
            let id = pool
                .submit(move || Ok((i * 2).to_le_bytes().to_vec()))
                .await
                .unwrap();
            ids.push(id);
        }

        // Collect results
        let mut results = Vec::new();
        for id in ids {
            let updates = pool.subscribe(id).await.unwrap();
            while let Ok(status) = updates.recv().await {
                if let JobStatus::Success(res) = status {
                    let result_u32 = u32::from_le_bytes(res.try_into().unwrap());
                    results.push(result_u32);
                    break;
                }
            }
        }

        // Verify results
        results.sort();
        assert_eq!(results, vec![0, 2, 4, 6, 8]);

        pool.shutdown().await;
    }

    #[tokio::test]
    async fn test_worker_pool_error_handling() {
        let pool = WorkerPool::new(2, 5, 2.0);

        let error_message = "Test error".to_string();
        let error_message_clone = error_message.clone(); // Clone for closure
        let (_id, receiver) = pool
            .submit_and_subscribe(move || Err(WorkError::InternalError(error_message_clone)))
            .await
            .unwrap();

        while let Ok(status) = receiver.recv().await {
            if let JobStatus::Error(e) = status {
                assert_eq!(e, WorkError::InternalError(error_message));
                break;
            }
        }

        pool.shutdown().await;
    }

    #[tokio::test]
    async fn test_worker_pool_cancellation() {
        let pool = WorkerPool::new(2, 5, 2.0);

        // Submit a slow work item using closure and get subscription
        let (id, receiver) = pool
            .submit_and_subscribe(move || {
                std::thread::sleep(Duration::from_millis(1000));
                Ok(vec![1, 2, 3])
            })
            .await
            .unwrap();

        // Cancel it immediately
        let cancelled_job = pool.cancel(id).await;
        assert!(cancelled_job.is_ok());

        // Verify the job was cancelled using subscription
        while let Ok(status) = receiver.recv().await {
            if matches!(status, JobStatus::Cancelled) {
                break;
            }
        }

        pool.shutdown().await;
    }

    #[tokio::test]
    async fn test_worker_pool_timeout() {
        let pool = WorkerPool::new(2, 5, 1.0); // 1 second timeout

        // Submit a work item that takes longer than the timeout using closure
        let id = pool
            .submit(move || {
                std::thread::sleep(Duration::from_millis(2000));
                Ok(vec![4, 5, 6])
            })
            .await
            .unwrap();

        // Wait for the timeout
        tokio::time::sleep(Duration::from_secs(2)).await;

        // The job should have been garbage collected
        assert!(pool.poll(id).await.is_none());

        pool.shutdown().await;
    }

    #[tokio::test]
    async fn test_worker_pool_concurrent_work() {
        let pool = WorkerPool::new(4, 10, 2.0);

        // Submit multiple slow work items using closures and get subscriptions
        let mut receivers = vec![];
        for _ in 0..4 {
            let (_, receiver) = pool
                .submit_and_subscribe(move || {
                    std::thread::sleep(Duration::from_millis(100));
                    Ok(vec![7, 8, 9])
                })
                .await
                .unwrap();
            receivers.push(receiver);
        }

        // All jobs should complete within a reasonable time
        let start = std::time::Instant::now();
        for receiver in receivers {
            JobStatus::wait_for_success(&receiver).await.unwrap();
        }
        let duration = start.elapsed();

        // Should take less than 500ms total (4 jobs * 100ms each, but parallel)
        assert!(duration < Duration::from_millis(500));

        pool.shutdown().await;
    }

    #[tokio::test]
    async fn test_too_many_jobs() {
        let pool = WorkerPool::new(2, 5, 5.0);

        for _ in 0..5 {
            let _id = pool
                .submit(move || {
                    std::thread::sleep(Duration::from_millis(5000));
                    Ok(vec![1])
                })
                .await
                .unwrap();
        }

        // Allow the 2 workers to pick up the jobs
        tokio::time::sleep(Duration::from_millis(200)).await;

        for _ in 0..2 {
            let _id = pool
                .submit(move || {
                    std::thread::sleep(Duration::from_millis(5000));
                    Ok(vec![2])
                })
                .await
                .unwrap();
        }

        // Should fail to submit the 8th job (capacity is 5 pending)
        let res = pool
            .submit(move || {
                std::thread::sleep(Duration::from_millis(5000));
                Ok(vec![3])
            })
            .await;
        assert!(res.is_err());
        assert!(matches!(res, Err(WorkerPoolError::JobQueueFull)));
    }
}
