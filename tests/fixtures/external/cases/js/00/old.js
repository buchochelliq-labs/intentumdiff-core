
const hasCloseElements = (numbers, threshold) => {
  for (let i = 0; i < numbers.length; i++) {
    for (let j = 0; j < numbers.length; j++) {
      if (i != j) {
        let distance = numbers[i] - numbers[j];
        if (distance < threshold) {
          return true;
        }
      }
    }
  }
  return false;
}

