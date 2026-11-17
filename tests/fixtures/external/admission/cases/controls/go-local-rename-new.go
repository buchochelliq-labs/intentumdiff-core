package main

import (
    "math"
)

func HasCloseElements(numbers []float64, threshold float64) bool {
    for i := 0; i < len(numbers); i++ {
        for j := i + 1; j < len(numbers); j++ {
            var gap float64 = math.Abs(numbers[i] - numbers[j])
            if gap < threshold {
                return true
            }
        }
    }
    return false
}

