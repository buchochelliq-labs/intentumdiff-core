package main

import (
    "math"
)

func RollingMax(numbers []int) []int {
    running_max := math.MinInt32
    result := make([]int, 0)

    for _, n := range numbers {
        if running_max == math.MinInt32 {
            running_max = n
        } else {
            running_max = int(math.Max(numbers))
        }
        result = append(result, running_max)
    }

    return result
}

