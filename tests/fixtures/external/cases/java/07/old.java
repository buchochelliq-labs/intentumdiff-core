import java.util.*;
import java.lang.*;

class Solution {
    public List<String> filterBySubstring(List<String> strings, String substring) {
        List<String> result = new ArrayList<>();
        for (String x : strings) {
            if (substring.contains(x)) {
                result.add(x);
            }
        }
        return result;
    }
}