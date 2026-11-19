use std::{
    collections::BTreeMap,
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

static ACTIVE: AtomicBool = AtomicBool::new(false);
static PROFILE: OnceLock<Mutex<Profile>> = OnceLock::new();

struct Profile {
    started: Instant,
    phases: BTreeMap<&'static str, (Duration, u64)>,
}

pub fn begin() {
    if std::env::var_os("FAITH_PROFILE_STARTUP").is_some_and(|value| value == "1") {
        let _ = PROFILE.set(Mutex::new(Profile {
            started: Instant::now(),
            phases: BTreeMap::new(),
        }));
        ACTIVE.store(true, Ordering::Relaxed);
    }
}

pub fn enabled() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

pub struct Span {
    name: &'static str,
    started: Option<Instant>,
}

impl Span {
    pub fn new(name: &'static str) -> Self {
        Self {
            name,
            started: ACTIVE.load(Ordering::Relaxed).then(Instant::now),
        }
    }
}

impl Drop for Span {
    fn drop(&mut self) {
        if let Some(started) = self.started
            && ACTIVE.load(Ordering::Relaxed)
            && let Some(profile) = PROFILE.get()
            && let Ok(mut profile) = profile.lock()
        {
            let phase = profile.phases.entry(self.name).or_default();
            phase.0 += started.elapsed();
            phase.1 += 1;
        }
    }
}

pub fn measure<T>(name: &'static str, operation: impl FnOnce() -> T) -> T {
    let _span = Span::new(name);
    operation()
}

pub fn finish() {
    if !ACTIVE.swap(false, Ordering::Relaxed) {
        return;
    }

    if let Some(profile) = PROFILE.get()
        && let Ok(profile) = profile.lock()
    {
        eprintln!(
            "Startup profile: {:.3} ms to first presented frame (nested timings overlap)",
            profile.started.elapsed().as_secs_f64() * 1000.0
        );
        let mut phases: Vec<_> = profile.phases.iter().collect();
        phases.sort_by_key(|(_, (elapsed, _))| std::cmp::Reverse(*elapsed));

        for (name, (elapsed, count)) in phases {
            eprintln!(
                "startup {name:28} {:10.3} ms  {count:5} calls",
                elapsed.as_secs_f64() * 1000.0
            );
        }
    }
}
