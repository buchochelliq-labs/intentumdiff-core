#include<stdio.h>
#include<vector>
#include<math.h>
using namespace std;
#include<algorithm>
#include<stdlib.h>
bool has_close_elements(vector<float> numbers, float threshold){
    int i,k;
    
    for (i=0;i<numbers.size();i++)
    for (k=i+1;k<numbers.size();k++)
    if (abs(numbers[i]-numbers[k])<threshold)
    return true;

    return false;
}

