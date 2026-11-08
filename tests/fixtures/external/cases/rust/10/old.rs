
use std::{slice::Iter, cmp::{max, self}, mem::replace, collections::{HashSet, HashMap}, ops::Index, ascii::AsciiExt};
use rand::Rng;
use regex::Regex;
use md5;
use std::any::{Any, TypeId};

fn is_palindrome_10(str: &str) -> bool {
        let s: String = str.chars().rev().collect();
        return s==str;
    }

fn make_palindrome(str: &str) -> String {

        let mut i: usize = 0;
        for i in 0..str.len() {
            let rstr: &str = &str[i..];
            if is_palindrome_10(str) {
                let nstr: &str = &str[0..i];
                let n2str: String = nstr.chars().rev().collect();
                return str.to_string()+&n2str;
            }
        }
        let n2str: String = str.chars().rev().collect();
        return str.to_string()+&n2str;
    }
