pub const MAX_PROGRESS_TICKS: u32 = 100_000;
pub const TICKS_PER_PERCENT: u32 = MAX_PROGRESS_TICKS / 100;

/// used to represent a progress percentage
/// internally keeps a progress value as ticks, where
/// ticks range from 0 to 100,000. every 1000 ticks
/// is 1%.
#[derive(Default)]
pub struct ProgressCompute {
    ticks: u32,
}

#[macro_export]
macro_rules! delegate_progresscompute_on {
    ($name:tt) => {
        delegate ! {
            to self.$name {
                pub fn set_progress(&mut self, new_progress: u32);
                pub fn set_progress_percent(&mut self, prog_percent: f64);
                pub fn set_progress_percent_normalized(&mut self, prog_norm: f64);
                pub fn inc_progress(&mut self, new_progress: u32);
                pub fn inc_progress_percent(&mut self, new_progress: f64);
                pub fn inc_progress_percent_normalized(&mut self, prog_norm: f64);
                pub fn get_progress_percent_normalized(&self) -> f64;
                pub fn get_progress_percent(&self) -> f64;
                pub fn get_progress(&self) -> u32;
            }
        }
    };
}

impl ProgressCompute {
    /// set the progress level. new_progress must be in 'ticks'
    /// where 1000 ticks represents 1%
    pub fn set_progress(&mut self, new_progress: u32) {
        if new_progress > MAX_PROGRESS_TICKS as u32 {
            self.ticks = MAX_PROGRESS_TICKS;
        } else {
            // this is safe to do because we checked if its over 100,000 which if its not
            // then it will definitely fit into u32
            self.ticks = new_progress;
        }
    }

    /// prog_percent is a float64 from 0.0-100.0 inclusively
    pub fn set_progress_percent(&mut self, prog_percent: f64) {
        if prog_percent < 0 as f64 {
            return;
        }

        let new_ticks = prog_percent * TICKS_PER_PERCENT as f64;
        self.set_progress(new_ticks as u32);
    }

    /// prog_norm is a float64 from 0.0-1.0 inclusively where 0.0
    /// represents 0%, and 1.0 represents 100%
    pub fn set_progress_percent_normalized(&mut self, prog_norm: f64) {
        self.set_progress_percent(prog_norm * 100.0);
    }

    /// like set_progress but only allows progress to increase
    pub fn inc_progress(&mut self, new_progress: u32) {
        if new_progress > self.ticks {
            self.set_progress(new_progress);
        }
    }

    /// like set_progress_percent but only allows increasing the progress
    pub fn inc_progress_percent(&mut self, new_progress: f64) {
        if new_progress < 0 as f64 {
            return;
        }

        let new_ticks = new_progress * TICKS_PER_PERCENT as f64;
        self.inc_progress(new_ticks as u32);
    }

    /// like set_progress_percent_normalized but only allows increasing
    pub fn inc_progress_percent_normalized(&mut self, prog_norm: f64) {
        self.inc_progress_percent(prog_norm * 100.0);
    }

    /// returns a value between 0 and 1 (inclusive) of the percentage
    /// normalized. ie: 1 <-> 100%, 0 <-> 0%
    pub fn get_progress_percent_normalized(&self) -> f64 {
        let percent_norm = self.ticks as f64 / MAX_PROGRESS_TICKS as f64;
        percent_norm
    }

    /// returns a value between 0.0 and 100.0 of the percentage
    pub fn get_progress_percent(&self) -> f64 {
        let percent_norm = self.get_progress_percent_normalized();
        percent_norm * 100.0
    }

    pub fn get_progress(&self) -> u32 { self.ticks }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_progress_percent_works() {
        let mut myprog = ProgressCompute::default();
        myprog.set_progress_percent(0.0);
        assert_eq!(myprog.get_progress(), 0);
        myprog.set_progress_percent(22.5);
        let expected_ticks = 22.5 * TICKS_PER_PERCENT as f64;
        let expected_ticks = expected_ticks as u32;
        assert_eq!(myprog.get_progress(), expected_ticks);
        myprog.set_progress_percent(99.9999999);
        assert_ne!(myprog.get_progress(), MAX_PROGRESS_TICKS);
    }

    #[test]
    fn inc_progress_works_percent() {
        let mut myprog = ProgressCompute::default();
        myprog.set_progress(TICKS_PER_PERCENT * 3);
        assert_eq!(myprog.get_progress(), TICKS_PER_PERCENT * 3);
        myprog.inc_progress(TICKS_PER_PERCENT * 10);
        assert_eq!(myprog.get_progress(), TICKS_PER_PERCENT * 10);
        myprog.inc_progress(TICKS_PER_PERCENT * 3);
        // it should still be 10%, cant go down with inc_progress
        assert_eq!(myprog.get_progress(), TICKS_PER_PERCENT * 10);
    }

    #[test]
    fn set_progress_works() {
        let mut myprog = ProgressCompute::default();
        myprog.set_progress(TICKS_PER_PERCENT);
        assert_eq!(myprog.get_progress(), TICKS_PER_PERCENT);
    }
}
