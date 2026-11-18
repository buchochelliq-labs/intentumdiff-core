module example.com/printversion

require example.com/version v1.0.0
replace example.com/version v1.0.0 => ../oops v0.0.0
exclude example.com/version v1.1.0
