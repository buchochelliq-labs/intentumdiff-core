
x = $(foreach x,y)
y := $x

all: ; @echo $y
