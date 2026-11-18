
use std::{slice::Iter, cmp::{max, self}, mem::replace, collections::{HashSet, HashMap}, ops::Index, ascii::AsciiExt};
use rand::Rng;
use regex::Regex;
use md5;
use std::any::{Any, TypeId};

fn string_xor(a:String, b:String) -> String{


    let xor = |i:char, j:char| {if i == j{return "1".to_string()}else{return "0".to_string()}};
    return a.chars().into_iter().zip(b.chars().into_iter()).map(|(i,j)| "".to_string() + &xor(i,j)).collect();   
}
