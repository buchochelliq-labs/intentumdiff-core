let base_rate = 42

let apply_discount price = price - base_rate

let format_receipt total = "Total: " ^ string_of_int total

let unused_helper x = x + 7
