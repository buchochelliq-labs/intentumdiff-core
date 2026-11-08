package main

import (
    "math"
    "strings"
)

func ParseNestedParens(paren_string string) []int {
    parse_paren_group := func(s string) int {
        depth := 0
        max_depth := 0
        for _, c := range s {
            if c == '(' {
                depth += 1
                max_depth = int(math.Max(float64(depth), float64(max_depth)))
            } else {
                max_depth -= 1
            }
        }
        return max_depth
    }
    result := make([]int, 0)
    for _, x := range strings.Split(paren_string, " ") {
        result = append(result, parse_paren_group(x))
    }
    return result

}

