use sample::greet;
use sample::Counter;

fn main() {
    let msg = greet("world");
    println!("{msg}");

    let mut c = Counter::new();
    c.bump();
}
