import java.util.*;
import java.lang.*;

class Solution {
    public boolean belowZero(List<Integer> operations) {
        int balance = 0;

        for (int op : operations) {
            balance += op;
            if (balance < 0) {
                return true;
            }
        }

        return false;
    }
}