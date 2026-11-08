
use std::{slice::Iter, cmp::{max, self}, mem::replace, collections::{HashSet, HashMap}, ops::Index, ascii::AsciiExt};
use rand::Rng;
use regex::Regex;
use md5;
use std::any::{Any, TypeId};

fn below_zero(operations:Vec<i32>) -> bool{


let mut balance:i32 = 0;
for op in operations {
    balance = balance + op;
    if balance < 0 {
        return true;
        }
    }
    return false;
}
