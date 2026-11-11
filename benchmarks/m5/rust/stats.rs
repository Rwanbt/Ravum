use std::env;
use std::fs;

fn main() -> Result<(), String> {
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        println!("usage: stats <file>");
        return Ok(());
    }
    let text = fs::read_to_string(&args[1]).map_err(|e| e.to_string())?;
    let mut sum = 0.0;
    let mut count = 0;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let value: f64 = trimmed.parse().map_err(|_| "invalid number".to_string())?;
        sum += value;
        count += 1;
    }
    if count == 0 {
        return Err("no values in file".to_string());
    }
    let mean = sum / (count as f64);
    println!("{}", mean);
    Ok(())
}
