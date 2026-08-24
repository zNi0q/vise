fn half(n: i64) -> Result<i64, &'static str> {
    if n % 2 == 0 { Ok(n / 2) } else { Err("odd") }
}

fn main() {
    let n: i64 = std::env::args().nth(1).map_or(50000000, |a| a.parse().unwrap());
    let mut sum: i64 = 0;
    let mut i: i64 = 0;
    while i < n {
        sum += match half(i) { Ok(v) => v, Err(_) => 0 };
        i += 1;
    }
    println!("{sum}");
}
