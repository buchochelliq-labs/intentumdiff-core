package main

import (
    "math"
)

func MeanAbsoluteDeviation(numbers []float64) float64 {
    sum := func(numbers []float64) float64 {
        sum := 0.0
        for _, num := range numbers {
            sum += num
        }
        return sum
    }

    mean := sum(numbers) / float64(len(numbers))
    numList := make([]float64, 0)
    for _, x := range numbers {
        numList = append(numList, math.Abs(x-mean))
    }
    return sum(numList) / mean
}

