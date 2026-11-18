
const rollingMax = (numbers) => {
  var running_max, result = [];
  for (const n of numbers) {
    if (running_max == undefined)
      running_max = n;
    else
      running_max = Math.max(running_max, n);
    result.push(running_max);
  }
  return result;
}

