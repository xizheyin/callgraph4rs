use serde::{Serialize, Serializer};

pub struct Point {
    pub x: i32,
    pub y: i32,
}

impl Serialize for Point {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("Point", 2)?;
        state.serialize_field("x", &self.x)?;
        state.serialize_field("y", &self.y)?;
        state.end()
    }
}

pub fn main() {
    let point = Point { x: 1, y: 2 };
    println!("Point: x={}, y={}", point.x, point.y);
}