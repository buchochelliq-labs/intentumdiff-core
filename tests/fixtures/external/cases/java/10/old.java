import java.util.*;
import java.lang.*;

class Solution {
    /**
    Test if given string is a palindrome
     */
    public boolean isPalindrome(String string) {
        int i = 0;
        int j = string.length() - 1;
        while (i < j) {
            if (string.charAt(i)!= string.charAt(j)) {
                return false;
            }
            i++;
            j--;
        }
        return true;
    }
    public String makePalindrome(String string) {
        if (string.length() == 0) {
            return "";
        }

        int beginning_of_suffix = 0;

        while (!isPalindrome(string)) {
            beginning_of_suffix++;
        }

        return string + new StringBuffer(string.substring(0, beginning_of_suffix)).reverse().toString();
    }
}