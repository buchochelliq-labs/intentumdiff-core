(module
  (func (export "value-i32") (result i32) (i32.const 77))
)
(assert_return (invoke "value-i32") (i32.const 77))
