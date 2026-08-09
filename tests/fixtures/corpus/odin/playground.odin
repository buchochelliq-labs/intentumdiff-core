package main

import "core:fmt"

greet :: proc(name: string) {
    fmt.printf("Hello, %s!\n", name)
}

add :: proc(a, b: int) -> int { return a + b }

multiply :: proc(a, b: int) -> int { return a * b }

main :: proc() {
    greet("World")
    fmt.println("add:", add(3, 4))
    fmt.println("mul:", multiply(3, 4))
}
