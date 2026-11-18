
const sumProduct = (numbers, int) => {
  var sum_value = 0, prod_value = 0;
  for (const n of numbers) {
    sum_value += n;
    prod_value *= n;
  }
  return [sum_value, prod_value];
}

