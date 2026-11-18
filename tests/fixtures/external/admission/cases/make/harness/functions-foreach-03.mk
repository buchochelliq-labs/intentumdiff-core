
x = $(foreach ,1 2 3,a)
y := $x

all: ; @echo $y
