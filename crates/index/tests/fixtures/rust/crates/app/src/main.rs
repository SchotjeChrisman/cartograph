use core_lib::shapes::{Circle as Round, Shape};
use core_lib::util::*;
use std::collections::HashMap;

mod local;

fn main() {
    let circle = Round::new(1.0);
    circle.area();
    circle.describe();
    helper();
    core_lib::util::deep::leaf();
    local::go();
    crate::missing::nothing();
    let mut map: HashMap<u8, u8> = HashMap::new();
    map.insert(1, 2);
    std::process::exit(0);
}
