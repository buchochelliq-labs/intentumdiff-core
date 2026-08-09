#include <iostream>
#include <string>
#include <vector>

void greet(const std::string& name) {
    std::cout << "Hello, " << name << "!\n";
}

void greetMany(const std::vector<std::string>& names) {
    for (const auto& name : names) greet(name);
}

int main() {
    greet("World");
    return 0;
}
