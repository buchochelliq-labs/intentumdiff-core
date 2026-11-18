
use std::{slice::Iter, cmp::{max, self}, mem::replace, collections::{HashSet, HashMap}, ops::Index, ascii::AsciiExt};
use rand::Rng;
use regex::Regex;
use md5;
use std::any::{Any, TypeId};

fn intersperse(numbers:Vec<u32>, delimeter: u32) -> Vec<u32>{

    let mut res:Vec<u32> = vec![];
    numbers.iter().for_each(|item:&u32| {res.push(*item); res.push(delimeter);});
    res.pop();
    return res;
}
