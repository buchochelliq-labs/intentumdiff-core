

X1 := $(sort $(filter FOO BAR,$(.VARIABLES)))

FOO := foo

X2 := $(sort $(filter FOO BAR,$(.VARIABLES)))

BAR := bar

all: ; @echo X1 = $(X1); echo X2 = $(X2); echo LAST = $(sort $(filter FOO BAR,$(.VARIABLES)))
