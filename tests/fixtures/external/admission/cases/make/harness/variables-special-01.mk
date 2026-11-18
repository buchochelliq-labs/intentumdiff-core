
FOO := foo
BAR := bar
$(info one: $(sort $(filter FOO BAR BAZ,$(.VARIABLES))))
undefine BAR
BAZ := baz
$(info two: $(sort $(filter FOO BAR BAZ,$(.VARIABLES))))
all:;@:
