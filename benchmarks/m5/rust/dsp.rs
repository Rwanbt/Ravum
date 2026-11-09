fn gain(samples: &[f64], g: f64) -> Vec<f64> {
    let mut out = Vec::with_capacity(samples.len());
    let mut i = 0;
    while i < samples.len() {
        out.push(samples[i] * g);
        i += 1;
    }
    out
}

fn iir(samples: &[f64], a: f64, b: f64) -> Vec<f64> {
    let mut out = Vec::with_capacity(samples.len());
    let mut y = 0.0;
    let mut i = 0;
    while i < samples.len() {
        y = a * y + b * samples[i];
        out.push(y);
        i += 1;
    }
    out
}

fn main() {
    let input = [1.0, 0.5, -0.25, 0.0];
    let boosted = gain(&input, 2.0);
    let filtered = iir(&input, 0.8, 0.2);
    for value in &boosted {
        println!("{}", value);
    }
    println!("---");
    for value in &filtered {
        println!("{}", value);
    }
}
