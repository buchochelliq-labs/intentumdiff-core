const isPalindrome = (string) => {
  return string == string.split('').reverse().join('');
}

const makePalindrome = (string) => {
  if (string == '')
    return '';
  var beginning_of_suffix = 0;
  while (!isPalindrome(string))
    beginning_of_suffix += 1;
  return string + string.slice(0, beginning_of_suffix).split('').reverse().join('');
}

