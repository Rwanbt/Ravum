fn fibonacci(n: u32) -> u64 {
    if n < 2 {
        return n as u64;
    }
    fibonacci(n - 1) + fibonacci(n - 2)
}

fn main() {
    for i in 0..=10 {
        println!("{}", fibonacci(i));
    }
}
