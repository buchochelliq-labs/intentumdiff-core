import os


def greet(name):
    print("Hello, " + name)
    return name


class Box:
    def __init__(self):
        self.value = 42

    def stub(self):
        pass


async def fetch(url):
    if url:
        return await load(url)
    for item in range(3):
        yield item
