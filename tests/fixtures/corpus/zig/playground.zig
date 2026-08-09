const std = @import("std");

fn add(a: i32, b: i32) i32 {
    return a + b;
}

fn multiply(a: i32, b: i32) i32 {
    return a * b;
}

pub fn main() void {
    std.debug.print("add: {d}\n", .{add(3, 4)});
    std.debug.print("mul: {d}\n", .{multiply(3, 4)});
}
