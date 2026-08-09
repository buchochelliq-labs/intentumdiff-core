public class Calculator {
    public int add(int first, int second) {
        return first + second;
    }

    public int multiply(int first, int second) {
        return first * second;
    }

    public double divide(int dividend, int divisor) {
        if (divisor == 0) throw new ArithmeticException("Division by zero");
        return (double) dividend / divisor;
    }
}
