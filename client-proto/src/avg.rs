/// An [Exponential moving average][ema] implementation based on libtorrent.
///
/// [ema]: https://blog.libtorrent.org/2014/09/running-averages/
///
#[derive(Default)]
pub struct MovingAverage {
    mean: isize,
    num_samples: isize,
    max_samples: isize,
}

impl MovingAverage {
    pub fn new(max_samples: isize) -> Self {
        assert!(max_samples > 0);
        Self {
            mean: 0,
            num_samples: 0,
            max_samples,
        }
    }

    pub fn add_sample(&mut self, mut sample: isize) {
        sample *= 64;

        if self.num_samples < self.max_samples {
            self.num_samples += 1;
        }

        self.mean += (sample - self.mean) / self.num_samples;
    }

    pub fn mean(&self) -> isize {
        if self.num_samples > 0 {
            (self.mean + 32) / 64
        } else {
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::MovingAverage;

    #[test]
    fn reaction_time() {
        let mut avg = MovingAverage::new(10);
        avg.add_sample(-10);
        avg.add_sample(10);

        assert_eq!(avg.mean(), 0);
    }

    #[test]
    fn reaction_time2() {
        let mut avg = MovingAverage::new(10);
        avg.add_sample(10);
        avg.add_sample(20);

        assert_eq!(avg.mean(), 15);
    }

    #[test]
    fn converge() {
        let mut avg = MovingAverage::new(10);
        avg.add_sample(100);
        for _ in 0..20 {
            avg.add_sample(10);
        }

        assert!((avg.mean() - 10).abs() <= 3);
    }

    #[test]
    fn converge2() {
        let mut avg = MovingAverage::new(10);
        avg.add_sample(-100);
        for _ in 0..20 {
            avg.add_sample(-10);
        }

        assert!((avg.mean() + 10).abs() <= 3);
    }

    #[test]
    fn random_converge() {
        let mut avg = MovingAverage::new(10);
        let samples = [
            49, 51, 60, 46, 65, 53, 76, 59, 57, 54, 56, 51, 45, 80, 53, 62, 69, 67, 66, 56, 56, 61,
            52, 61, 61, 62, 59, 53, 48, 68, 47, 47, 63, 51, 53, 54, 46, 65, 64, 64, 45, 68, 64, 66,
            53, 42, 57, 58, 57, 47, 55, 59, 64, 61, 37, 67, 55, 52, 60, 60, 44, 57, 50, 77, 56, 54,
            49, 68, 66, 64, 47, 60, 46, 47, 81, 74, 65, 62, 44, 75, 65, 43, 58, 59, 53, 67, 49, 51,
            33, 47, 49, 50, 54, 48, 55, 80, 67, 51, 66, 52, 48, 57, 30, 51, 72, 65, 78, 56, 74, 68,
            49, 66, 63, 57, 61, 62, 64, 62, 61, 52, 67, 64, 59, 61, 69, 60, 54, 69,
        ];
        for &s in samples.iter() {
            avg.add_sample(s);
        }

        assert!((avg.mean() - 60).abs() <= 3);
    }
}
