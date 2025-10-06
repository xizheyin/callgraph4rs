use serde::Serialize;

#[derive(Serialize)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

pub fn main() {
    let point = Point { x: 1, y: 2 };
    println!("Point: x={}, y={}", point.x, point.y);
}