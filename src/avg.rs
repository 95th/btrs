#[derive(Default)]
pub struct SlidingAvg {
    mean: i32,
    avg_deviation: i32,
    num_samples: i32,
    inverted_gain: i32,
}

impl SlidingAvg {
    pub fn new(inverted_gain: i32) -> Self {
        assert!(inverted_gain > 0);
        Self {
            mean: 0,
            avg_deviation: 0,
            num_samples: 0,
            inverted_gain,
        }
    }

    pub fn add_sample(&mut self, mut s: i32) {
        s *= 64;
        let deviation = if self.num_samples > 0 {
            (self.mean - s).abs()
        } else {
            0
        };

        if self.num_samples < self.inverted_gain {
            self.num_samples += 1;
        }

        self.mean += (s - self.mean) / self.num_samples;

        if self.num_samples > 1 {
            self.avg_deviation += (deviation - self.avg_deviation) / (self.num_samples - 1);
        }
    }

    pub fn mean(&self) -> i32 {
        if self.num_samples > 0 {
            (self.mean + 32) / 64
        } else {
            0
        }
    }

    pub fn avg_deviation(&self) -> i32 {
        if self.num_samples > 1 {
            (self.avg_deviation + 32) / 64
        } else {
            0
        }
    }

    pub fn num_samples(&self) -> i32 {
        self.num_samples
    }
}

#[cfg(test)]
mod tests {
    use super::SlidingAvg;

    #[test]
    fn reaction_time() {
        let mut avg = SlidingAvg::new(10);
        avg.add_sample(-10);
        avg.add_sample(10);

        assert_eq!(avg.mean(), 0);
    }

    #[test]
    fn reaction_time2() {
        let mut avg = SlidingAvg::new(10);
        avg.add_sample(10);
        avg.add_sample(20);

        assert_eq!(avg.mean(), 15);
    }

    #[test]
    fn converge() {
        let mut avg = SlidingAvg::new(10);
        avg.add_sample(100);
        for _ in 0..20 {
            avg.add_sample(10);
        }

        assert!((avg.mean() - 10).abs() <= 3);
    }

    #[test]
    fn converge2() {
        let mut avg = SlidingAvg::new(10);
        avg.add_sample(-100);
        for _ in 0..20 {
            avg.add_sample(-10);
        }

        assert!((avg.mean() + 10).abs() <= 3);
    }

    #[test]
    fn random_converge() {
        let mut avg = SlidingAvg::new(10);
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

    #[test]
    fn sliding_average() {
        let mut avg = SlidingAvg::new(4);
        assert_eq!(avg.mean(), 0);
        assert_eq!(avg.avg_deviation(), 0);
        avg.add_sample(500);
        assert_eq!(avg.mean(), 500);
        assert_eq!(avg.avg_deviation(), 0);
        avg.add_sample(501);
        assert_eq!(avg.avg_deviation(), 1);
        avg.add_sample(0);
        avg.add_sample(0);
        assert!((avg.mean() - 250).abs() < 50);
        assert!((avg.avg_deviation() - 250).abs() < 80);
    }
}
