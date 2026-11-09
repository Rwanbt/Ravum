//! Binary target: analysed through the Cargo fallback (never a targeted HIT).

fn main() {
    println!("{}", consumer::run(2.0));
}
