namespace Demo {
    open Microsoft.Quantum.Intrinsic;
    open Microsoft.Quantum.Canon;

    operation SayHello(name : String) : Unit {
        Message($"Hello, {name}!");
    }

    operation FlipBit() : Result {
        use q = Qubit();
        X(q);
        let result = M(q);
        Reset(q);
        return result;
    }
}
