
use std::{slice::Iter, cmp::{max, self}, mem::replace, collections::{HashSet, HashMap}, ops::Index, ascii::AsciiExt};
use rand::Rng;
use regex::Regex;
use md5;
use std::any::{Any, TypeId};

fn sum_product(numbers:Vec<i32>) -> (i32,i32){

    let sum = |xs: &Vec<i32>| xs.iter()
    .fold(0, |mut sum, &val| { sum += val; 
                                                 sum }
    );
    let product = |xs: &Vec<i32>| xs.iter()
    .fold(1, |mut prod, &val| { prod *= val; 
                                                 prod }
    );
    return (sum(&numbers),product(&numbers));
}
