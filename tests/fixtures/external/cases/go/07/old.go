package main

import (
    "strings"
)

func FilterBySubstring(stringList []string, substring string) []string {
    result := make([]string, 0)
    for _, x := range stringList {
        if strings.Index(substring, x) != -1 {
            result = append(result, x)
        }
    }
    return result
}

