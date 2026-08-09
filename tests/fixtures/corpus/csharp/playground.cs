using System;

class Greeter {
    public void Greet(string name) {
        Console.WriteLine($"Hello, {name}!");
    }

    public void GreetAll(string[] names) {
        foreach (var name in names) Greet(name);
    }

    const string Salutation = "Greetings";
}
