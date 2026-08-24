fn main() {
    let n: i64 = std::env::args().nth(1).map_or(20000000, |a| a.parse().unwrap());
    let mut xs: Vec<i64> = Vec::new();
    for i in 0..n {
        xs.push(i);
    }
    let mut sum: i64 = 0;
    for j in 0..xs.len() {
        sum += xs[j];
    }
    println!("{sum}");
}
