use std::collections::VecDeque;
use std::time::Duration;

pub(super) struct FrameStats {
    pub visible: bool,
    samples: VecDeque<Duration>,
    total: Duration,
    frames: u64,
    pub slow: VecDeque<(u64, Duration)>,
    pub interval: Duration,
}

impl Default for FrameStats {
    fn default() -> Self {
        Self {
            visible: false,
            samples: VecDeque::with_capacity(120),
            total: Duration::ZERO,
            frames: 0,
            slow: VecDeque::with_capacity(8),
            interval: Duration::ZERO,
        }
    }
}

impl FrameStats {
    pub fn record(&mut self, elapsed: Duration, interval: Duration) {
        self.frames += 1;
        self.interval = interval;
        if self.samples.len() == 120 {
            self.total -= self.samples.pop_front().unwrap();
        }

        self.samples.push_back(elapsed);
        self.total += elapsed;
        if elapsed > interval {
            if self.slow.len() == 8 {
                self.slow.pop_front();
            }

            self.slow.push_back((self.frames, elapsed));
        }
    }

    pub fn average_ms(&self) -> f64 {
        self.total.as_secs_f64() * 1000.0 / self.samples.len().max(1) as f64
    }
}
