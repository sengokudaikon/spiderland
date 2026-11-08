//! The showcase's own pool: a few threads that run jobs in turn. The framework creates no tasks, so an app brings one like it.

use spiderland::Job;
use spiderland_slint::Background;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::JoinHandle;

/// Threads that run jobs. Dropping it lets the jobs it took finish, then stops the threads.
#[derive(Debug)]
pub struct Pool {
    jobs: Option<mpsc::Sender<Job>>,
    threads: Vec<JoinHandle<()>>,
    refusing: Arc<AtomicBool>,
}

impl Pool {
    /// A pool of `threads` threads.
    pub fn new(threads: usize) -> Pool {
        let (jobs, taken) = mpsc::channel::<Job>();
        let taken = Arc::new(Mutex::new(taken));
        let threads = (0..threads)
            .map(|_| {
                let taken = taken.clone();
                std::thread::spawn(move || {
                    loop {
                        let job = taken.lock().unwrap().recv();
                        let Ok(job) = job else {
                            break;
                        };
                        // A panic has reached the feature as an error by now. The thread goes on to the next job.
                        let _ = catch_unwind(AssertUnwindSafe(job));
                    }
                })
            })
            .collect();
        Pool {
            jobs: Some(jobs),
            threads,
            refusing: Arc::default(),
        }
    }

    /// Starts work on this pool, for the hosts.
    pub fn background(&self) -> Background {
        let jobs = self.jobs.clone().expect("the pool is open");
        let refusing = self.refusing.clone();
        Background::new(move |job| !refusing.load(Ordering::SeqCst) && jobs.send(job).is_ok())
    }

    /// Turns work away while `refuse` holds, as a full pool would.
    pub fn refuse(&self, refuse: bool) {
        self.refusing.store(refuse, Ordering::SeqCst);
    }
}

impl Drop for Pool {
    fn drop(&mut self) {
        drop(self.jobs.take());
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}
