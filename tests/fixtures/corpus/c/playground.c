#include <stdbool.h>
#include <stdio.h>

void greet(const char *name, bool excited) {
    printf("Hello, %s%s\n", name, excited ? "!" : "");
}

int main(void) {
    greet("World", true);
    return 0;
}
