package main

import (
    "strings"
)

func MakePalindrome(str string) string {
    if strings.TrimSpace(str) == "" {
        return ""
    }
    beginning_of_suffix := 0
    runes := []rune(str)
    for !IsPalindrome(string(runes[beginning_of_suffix:])) {
        beginning_of_suffix += 1
    }
    result := make([]rune, 0)
    for i := len(str[:beginning_of_suffix]) - 1; i >= 0; i-- {
        result = append(result, runes[i])
    }
    return str + string(result)
}

