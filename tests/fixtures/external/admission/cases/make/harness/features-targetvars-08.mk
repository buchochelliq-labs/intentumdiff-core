
VAR := $$FOO
foo: VAR += BAR
foo: ; @echo '$(VAR)'
