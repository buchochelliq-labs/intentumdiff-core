package main

import (
    "strings"
)

func FilterBySubstring(stringList []string, substring string) []string {
    result := make([]string, 0)
    for _, x := range stringList {
        if strings.Index(x, substring) != -1 {
            result = append(result, x)
        }
    }
    return result
}

