// 动态分派示例
// 这个例子展示了如何使用 dyn trait 进行动态分派

// 定义一个 trait
trait Signal {
    fn sample(&self, time: f64) -> f64;
    fn name(&self) -> &str;
}

// 实现 Signal trait 的具体类型
struct SineWave {
    frequency: f64,
    amplitude: f64,
}

impl SineWave {
    fn new(frequency: f64, amplitude: f64) -> Self {
        Self {
            frequency,
            amplitude,
        }
    }
}

impl Signal for SineWave {
    fn sample(&self, time: f64) -> f64 {
        self.amplitude * (2.0 * std::f64::consts::PI * self.frequency * time).sin()
    }

    fn name(&self) -> &str {
        "SineWave"
    }
}

struct SquareWave {
    frequency: f64,
    amplitude: f64,
}

impl SquareWave {
    fn new(frequency: f64, amplitude: f64) -> Self {
        Self {
            frequency,
            amplitude,
        }
    }
}

impl Signal for SquareWave {
    fn sample(&self, time: f64) -> f64 {
        let sine_val = (2.0 * std::f64::consts::PI * self.frequency * time).sin();
        if sine_val >= 0.0 {
            self.amplitude
        } else {
            -self.amplitude
        }
    }

    fn name(&self) -> &str {
        "SquareWave"
    }
}

struct NoiseGenerator {
    amplitude: f64,
}

impl NoiseGenerator {
    fn new(amplitude: f64) -> Self {
        Self { amplitude }
    }
}

impl Signal for NoiseGenerator {
    fn sample(&self, _time: f64) -> f64 {
        // 简单的伪随机噪声
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        ((_time * 1000.0) as u64).hash(&mut hasher);
        let random_val = (hasher.finish() % 1000) as f64 / 1000.0;
        self.amplitude * (random_val * 2.0 - 1.0)
    }

    fn name(&self) -> &str {
        "NoiseGenerator"
    }
}

// 使用 Box<dyn Signal> 进行动态分派
fn process_signal(signal: Box<dyn Signal>, duration: f64, sample_rate: f64) -> Vec<f64> {
    let mut samples = Vec::new();
    let num_samples = (duration * sample_rate) as usize;

    for i in 0..num_samples {
        let time = i as f64 / sample_rate;
        // 这里发生动态分派调用
        let sample = signal.sample(time);
        samples.push(sample);
    }

    println!("Processed {} samples from {}", samples.len(), signal.name());
    samples
}

// 使用 &dyn Signal 进行动态分派
fn analyze_signal(signal: &dyn Signal, time_points: &[f64]) -> (f64, f64) {
    let mut min_val = f64::INFINITY;
    let mut max_val = f64::NEG_INFINITY;

    for &time in time_points {
        // 这里也发生动态分派调用
        let sample = signal.sample(time);
        min_val = min_val.min(sample);
        max_val = max_val.max(sample);
    }

    println!(
        "Analyzed signal: {} (range: {} to {})",
        signal.name(),
        min_val,
        max_val
    );
    (min_val, max_val)
}

// 使用 Vec<Box<dyn Signal>> 处理多个信号
fn mix_signals(signals: Vec<Box<dyn Signal>>, time: f64) -> f64 {
    let mut result = 0.0;

    for signal in &signals {
        // 每次循环都会发生动态分派
        result += signal.sample(time);
    }

    result / signals.len() as f64
}

// 使用 Rc<dyn Signal> 的例子
use std::rc::Rc;

fn share_signal(signal: Rc<dyn Signal>, consumers: usize) -> Vec<f64> {
    let mut results = Vec::new();

    for i in 0..consumers {
        let time = i as f64 * 0.1;
        // 通过 Rc 进行动态分派
        let sample = signal.sample(time);
        results.push(sample);
        println!(
            "Consumer {} got sample {} from {}",
            i,
            sample,
            signal.name()
        );
    }

    results
}

// 主函数展示各种动态分派场景
pub fn main() {
    println!("=== 动态分派示例 ===");

    // 1. Box<dyn Signal> 示例
    println!("\n1. Box<dyn Signal> 示例:");
    let sine = Box::new(SineWave::new(440.0, 1.0)) as Box<dyn Signal>;
    let square = Box::new(SquareWave::new(220.0, 0.8)) as Box<dyn Signal>;
    let noise = Box::new(NoiseGenerator::new(0.3)) as Box<dyn Signal>;

    let _sine_samples = process_signal(sine, 0.1, 44100.0);
    let _square_samples = process_signal(square, 0.1, 44100.0);
    let _noise_samples = process_signal(noise, 0.1, 44100.0);

    // 2. &dyn Signal 示例
    println!("\n2. &dyn Signal 示例:");
    let sine2 = SineWave::new(880.0, 0.5);
    let square2 = SquareWave::new(110.0, 0.7);

    let time_points = vec![0.0, 0.001, 0.002, 0.003, 0.004];
    let _sine_range = analyze_signal(&sine2, &time_points);
    let _square_range = analyze_signal(&square2, &time_points);

    // 3. Vec<Box<dyn Signal>> 示例
    println!("\n3. 信号混合示例:");
    let signals: Vec<Box<dyn Signal>> = vec![
        Box::new(SineWave::new(440.0, 0.3)),
        Box::new(SquareWave::new(880.0, 0.2)),
        Box::new(NoiseGenerator::new(0.1)),
    ];

    let mixed_sample = mix_signals(signals, 0.001);
    println!("Mixed sample at t=0.001: {}", mixed_sample);

    // 4. Rc<dyn Signal> 示例
    println!("\n4. Rc<dyn Signal> 共享示例:");
    let shared_signal = Rc::new(SineWave::new(1000.0, 1.0)) as Rc<dyn Signal>;
    let _shared_results = share_signal(shared_signal, 3);

    println!("\n=== 动态分派示例完成 ===");
}

// 额外的函数式编程风格示例
fn functional_style_example() {
    println!("\n=== 函数式风格动态分派 ===");

    let signals: Vec<Box<dyn Signal>> = vec![
        Box::new(SineWave::new(440.0, 1.0)),
        Box::new(SquareWave::new(220.0, 0.8)),
        Box::new(NoiseGenerator::new(0.5)),
    ];

    // 使用闭包和动态分派
    let time = 0.001;
    let samples: Vec<f64> = signals
        .iter()
        .map(|signal| signal.sample(time)) // 动态分派发生在这里
        .collect();

    println!("Samples at t={}: {:?}", time, samples);

    // 找到最大振幅的信号
    let max_signal = signals.iter().max_by(|a, b| {
        let a_sample = a.sample(time).abs(); // 动态分派
        let b_sample = b.sample(time).abs(); // 动态分派
        a_sample.partial_cmp(&b_sample).unwrap()
    });

    if let Some(signal) = max_signal {
        println!("Signal with max amplitude: {}", signal.name());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dynamic_dispatch() {
        let signal: Box<dyn Signal> = Box::new(SineWave::new(440.0, 1.0));
        let sample = signal.sample(0.0);
        assert_eq!(sample, 0.0); // sin(0) = 0

        let signal: Box<dyn Signal> = Box::new(SquareWave::new(440.0, 1.0));
        let sample = signal.sample(0.0);
        assert_eq!(sample, 1.0); // square wave at 0 should be positive
    }

    #[test]
    fn test_trait_object_vec() {
        let signals: Vec<Box<dyn Signal>> = vec![
            Box::new(SineWave::new(440.0, 1.0)),
            Box::new(SquareWave::new(440.0, 1.0)),
        ];

        assert_eq!(signals.len(), 2);

        // 测试动态分派
        for signal in &signals {
            let _sample = signal.sample(0.0);
            let _name = signal.name();
        }
    }
}
