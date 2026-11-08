package main

import (
    "math"
)

func TruncateNumber(number float64) float64 {
    return math.Mod(number,1)
}

