fn square(n: i32) -> i32 {
    n.pow(2)
}

fn cube(n: i32) -> i32 {
    n.pow(3)
}

fn main() {
    println!("square: {}", square(5));
    println!("cube:   {}", cube(3));
}
