
arg1 = first
arg2 = second
arg3 = third
arg4 = cc
arg5 = fifth

result =

ifeq ($(arg1),$(arg2))
  result += arg1 equals arg2
else ifeq '$(arg2)' "$(arg5)"
  result += arg2 equals arg5
else ifneq '$(arg3)' '$(arg3)'
  result += arg3 NOT equal arg4
else ifndef arg5
  result += variable is undefined
else ifdef undefined
  result += arg4 is defined
else
  result += success
endif


all: ; @echo $(result)
