
all : A B
all : ; @echo $@ -- $^ -- $<
all : C D
all : E F
A B C D E F G H : ; @:
