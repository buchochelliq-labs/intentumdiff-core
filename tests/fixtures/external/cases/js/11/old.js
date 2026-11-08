
const stringXor = (a, b) => {
  var xor = function (i, j) {
    if (i == j)
      return '1';
    else
      return '0';
  }
  return a.split('').map((item, index) => xor(item, b[index])).join('');
}

