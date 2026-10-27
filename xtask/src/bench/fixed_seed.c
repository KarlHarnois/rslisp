#include <string.h>
#include <sys/types.h>

ssize_t getrandom(void *buffer, size_t length, unsigned int flags) {
    memset(buffer, 0, length);
    return (ssize_t)length;
}
