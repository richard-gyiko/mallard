pub mod greet;

pub use greet::greet;

pub fn double(x: i64) -> i64 {
    x * 2
}

pub struct Counter {
    count: i64,
}

impl Counter {
    pub fn new() -> Self {
        Counter { count: 0 }
    }

    pub fn bump(&mut self) {
        self.count += 1;
        let _ = double(self.count);
    }
}
