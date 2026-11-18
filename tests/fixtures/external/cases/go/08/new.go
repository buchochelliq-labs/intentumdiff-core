package main


func SumProduct(numbers []int) [2]int {
    sum_value := 0
    prod_value := 1

    for _, n := range numbers {
        sum_value += n
        prod_value *= n
    }
    return [2]int{sum_value, prod_value}
}

