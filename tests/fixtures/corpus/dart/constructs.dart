int add(int a, int b) {
  return a + b;
}

class Box {
  final int value = 42;
  String label() => "Hello, World!";
}

void main() {
  final result = add(1, 2);
  print(result);
}
