
const meanAbsoluteDeviation = (numbers) => {
  var mean = numbers.reduce((prev, item) => {
    return prev + item;
  }, 0) / numbers.length;
  return numbers.reduce((prev, item) => {
    return prev + Math.abs(item - mean);
  }, 0) / numbers.length;

}

