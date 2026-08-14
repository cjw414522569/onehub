#include <stddef.h>

int main(void) {
    const char *approved_bridge = "abi-c";
    const char *status = "interface-only";
    return (approved_bridge[0] != '\0' && status[0] != '\0') ? 0 : 1;
}
