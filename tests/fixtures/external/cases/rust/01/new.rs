
use std::{slice::Iter, cmp::{max, self}, mem::replace, collections::{HashSet, HashMap}, ops::Index, ascii::AsciiExt};
use rand::Rng;
use regex::Regex;
use md5;
use std::any::{Any, TypeId};

fn separate_paren_groups(paren_string: String) -> Vec<String>{

    let mut result:Vec<String> = vec![];
    let mut current_string:String = String::new();
    let mut current_depth:u32 = 0;

    for c in paren_string.chars(){
        if c == '('{
            current_depth += 1;
            current_string.push(c);
        }
        else if c == ')' {
            current_depth -= 1;
            current_string.push(c);

            if current_depth == 0{
                result.push(current_string.clone());
                current_string.clear()
            }
            
        }


    }
    return result;
}
