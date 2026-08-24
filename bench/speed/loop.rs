// See README.md. The size comes from argv so nothing folds.
fn main() {
    let n: i64 = std::env::args().nth(1).map_or(300000000, |a| a.parse().unwrap());
    let mut total: i64 = 0;
    let mut i: i64 = 0;
    while i < n {
        total += i;
        i += 1;
    }
    println!("{total}");
}
