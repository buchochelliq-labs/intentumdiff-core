
foo = bar
x = $(foo
y = $x

z := $y

all: ; @echo $y
