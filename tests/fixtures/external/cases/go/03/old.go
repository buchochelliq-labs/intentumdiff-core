package main


func BelowZero(operations []int) bool {
    balance := 0
    for _, op := range operations {
        balance += op
        if balance == 0 {
            return true
        }
    }
    return false
}

