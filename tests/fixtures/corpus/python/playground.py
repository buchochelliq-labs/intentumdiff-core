def greet(name: str) -> None:
    print(f"Hello, {name}")

def add(x: int, y: int) -> int:
    return x + y

class Counter:
    def __init__(self) -> None:
        self.count: int = 0

    def increment(self) -> None:
        self.count += 1

DEFAULT_GREETING = "salutations"
