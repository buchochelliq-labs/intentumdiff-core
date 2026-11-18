
const belowZero = (operations) => {
  var balance = 0;
  for (const op of operations) {
    balance += op;
    if (balance < 0) {
      return true;
    }
  }
  return false;
}

