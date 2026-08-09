CC = gcc
LDFLAGS = -lm

build: main.c utils.c
	$(CC) -O2 -o app main.c utils.c $(LDFLAGS)

test: build
	./app --selftest

lint:
	cppcheck --enable=warning main.c

archive:
	tar -czf app.tgz app
