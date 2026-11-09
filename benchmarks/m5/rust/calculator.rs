use std::env;

fn apply(a: f64, op: &str, b: f64) -> Result<f64, String> {
    match op {
        "+" => Ok(a + b),
        "-" => Ok(a - b),
        "*" => Ok(a * b),
        "/" => {
            if b == 0.0 {
                Err("division by zero".to_string())
            } else {
                Ok(a / b)
            }
        }
        _ => Err("unknown operator".to_string()),
    }
}

fn main() -> Result<(), String> {
    let args: Vec<String> = env::args().collect();
    if args.len() != 4 {
        println!("usage: calculator <a> <op> <b>");
        return Ok(());
    }
    let a: f64 = args[1].parse().map_err(|_| "invalid number".to_string())?;
    let b: f64 = args[3].parse().map_err(|_| "invalid number".to_string())?;
    let result = apply(a, &args[2], b)?;
    println!("{}", result);
    Ok(())
}
