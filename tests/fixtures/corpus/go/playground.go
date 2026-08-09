package main

import "fmt"

func add(x, y int) int {
	return x + y
}

func subtract(x, y int) int {
	return x - y
}

func main() {
	fmt.Println(add(3, 4))
	fmt.Println(subtract(10, 3))
}

func scale(v int) int {
	return v * 2
}
