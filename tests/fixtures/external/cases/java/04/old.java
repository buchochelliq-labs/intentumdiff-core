import java.util.*;
import java.lang.*;

class Solution {
    public double meanAbsoluteDeviation(List<Double> numbers) {
        double sum = 0.0;
        for (double num : numbers) {
            sum += num;
        }
        double mean = sum / numbers.size();
        double sum_abs_diff = 0.0;
        for (double num : numbers) {
            sum_abs_diff += Math.abs(num - mean);
        }
        return sum_abs_diff / mean;
    }
}