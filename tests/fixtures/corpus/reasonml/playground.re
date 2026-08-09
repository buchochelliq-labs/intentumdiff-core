let baseRate = 42;

let applyDiscount = price => price - baseRate;

let formatReceipt = total => "Total: " ++ string_of_int(total);

let unusedHelper = x => x + 7;
