// Resource limits: global semaphore + per-IP cap.
// Uses atomics for stats and a simple Mutex<HashMap> (acceptable given the hard caps).

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio::time::sleep;

#[derive(Clone)]
pub struct Limits {
    sem: Arc<Semaphore>,
    per_ip: Arc<Mutex<HashMap<IpAddr, u32>>>,
    per_ip_max: u32,
    pub total: Arc<AtomicU64>,
    pub held: Arc<AtomicU64>,
}

impl Limits {
    pub fn new(max_conns: usize, per_ip_max: u32) -> Self {
        Limits {
            sem: Arc::new(Semaphore::new(max_conns)),
            per_ip: Arc::new(Mutex::new(HashMap::new())),
            per_ip_max,
            total: Arc::new(AtomicU64::new(0)),
            held: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Try to acquire a slot. Returns the permit if allowed (global + per-IP).
    /// Caller must drop the permit (or pass to hold) and call release on drop.
    pub fn try_acquire(&self, ip: IpAddr) -> Option<OwnedSemaphorePermit> {
        let permit = match self.sem.clone().try_acquire_owned() {
            Ok(p) => p,
            Err(_) => return None, // global cap hit: shed
        };

        {
            let mut m = self.per_ip.lock().unwrap();
            let c = m.entry(ip).or_insert(0);
            if *c >= self.per_ip_max {
                drop(permit);
                return None; // one IP can't hog all slots
            }
            *c += 1;
        }

        self.total.fetch_add(1, Ordering::Relaxed);
        self.held.fetch_add(1, Ordering::Relaxed);
        Some(permit)
    }

    /// Release per-IP counter. Call after the connection ends.
    pub fn release(&self, ip: IpAddr) {
        self.held.fetch_sub(1, Ordering::Relaxed);
        let mut m = self.per_ip.lock().unwrap();
        if let Some(c) = m.get_mut(&ip) {
            *c = c.saturating_sub(1);
            if *c == 0 {
                m.remove(&ip);
            }
        }
    }

    /// Spawn a background task that prints periodic stats to stderr (for journald).
    pub fn spawn_status_task(&self) {
        let held = self.held.clone();
        let total = self.total.clone();
        tokio::spawn(async move {
            loop {
                sleep(Duration::from_secs(60)).await;
                eprintln!(
                    "[tinypit] held={} total={}",
                    held.load(Ordering::Relaxed),
                    total.load(Ordering::Relaxed)
                );
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};
    use std::sync::atomic::Ordering;

    fn test_ip(n: u8) -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(203, 0, 113, n))
    }

    #[test]
    fn test_per_ip_limit() {
        let limits = Limits::new(100, 2);

        let ip = test_ip(1);
        assert!(limits.try_acquire(ip).is_some());
        assert!(limits.try_acquire(ip).is_some());
        assert!(limits.try_acquire(ip).is_none()); // at per_ip_max

        limits.release(ip);
        limits.release(ip);
        // after release should allow again
        assert!(limits.try_acquire(ip).is_some());
    }

    #[test]
    fn test_global_cap() {
        let limits = Limits::new(1, 100);

        let ip1 = test_ip(1);
        let ip2 = test_ip(2);

        assert!(limits.try_acquire(ip1).is_some());
        assert!(limits.try_acquire(ip2).is_none()); // global exhausted
    }

    #[test]
    fn test_counters_and_cleanup() {
        let limits = Limits::new(10, 5);
        let ip = test_ip(7);

        let _p = limits.try_acquire(ip).unwrap();
        assert_eq!(limits.held.load(Ordering::Relaxed), 1);
        assert_eq!(limits.total.load(Ordering::Relaxed), 1);

        limits.release(ip);
        assert_eq!(limits.held.load(Ordering::Relaxed), 0);
    }
}
