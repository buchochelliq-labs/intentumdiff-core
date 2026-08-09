%let target = World;

data work.greeting;
  name    = "&target";
  suffix  = "!";
  message = cats("Hello, ", name, suffix);
  length  = lengthn(message);
  put message= length=;
run;

data work.scratch;
  attempts = 4;
run;

data work.audit;
  checked = 9;
run;

proc print data=work.greeting; run;
