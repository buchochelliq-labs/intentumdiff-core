(module
  (func $add (param $a i32) (param $b i32) (result i32)
    local.get $a
    local.get $b
    i32.add)
  (func $multiply (param $a i32) (param $b i32) (result i32)
    local.get $a
    local.get $b
    i32.mul)
  (func $scratch (param $n i32) (result i32)
    local.get $n)
  (func $twice (param $n i32) (result i32)
    local.get $n
    local.get $n
    i32.mul)
  (export "add" (func $add))
  (export "multiply" (func $multiply)))
