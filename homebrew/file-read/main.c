// SPDX-License-Identifier: MIT
// Guest de teste, não parte do runtime Rust. Somente APIs públicas do nxdk.
#include <windows.h>
#include <hal/video.h>
#include <hal/debug.h>
#include <xboxkrnl/xboxkrnl.h>
#include <stdint.h>

_Static_assert(sizeof(OBJECT_ATTRIBUTES) == 12, "Xbox object ABI");
_Static_assert(sizeof(IO_STATUS_BLOCK) == 8, "Xbox IOSB ABI");
_Static_assert(sizeof(FILE_NETWORK_OPEN_INFORMATION) == 56, "Xbox file ABI");
static unsigned char buffer[1024];
static void fail(const char *message) {
    debugPrint("FILE READ FAILED: %s\n", message);
    for (;;) Sleep(2000);
}
static HANDLE open_image(void) {
    HANDLE h = CreateFileA("D:\\default.xbe", GENERIC_READ, FILE_SHARE_READ,
                           NULL, OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, NULL);
    if (h == INVALID_HANDLE_VALUE) fail("CreateFileA");
    return h;
}
static uint32_t hash_bytes(uint32_t hash, const unsigned char *bytes, DWORD count) {
    for (DWORD i = 0; i < count; ++i) hash = (hash ^ bytes[i]) * 16777619u;
    return hash;
}
int main(void) {
    XVideoSetMode(640,480,32,REFRESH_DEFAULT);
    HANDLE h = open_image();
    LARGE_INTEGER size;
    if (!GetFileSizeEx(h,&size) || size.HighPart != 0) fail("GetFileSizeEx");
    DWORD got, total = 0;
    uint32_t hash = 2166136261u;
    if (!ReadFile(h,buffer,4,&got,NULL) || got != 4 || buffer[0] != 'X' ||
        buffer[1] != 'B' || buffer[2] != 'E' || buffer[3] != 'H') fail("XBEH");
    hash = hash_bytes(hash,buffer,got); total += got;
    do {
        if (!ReadFile(h,buffer,sizeof(buffer),&got,NULL)) fail("ReadFile");
        hash = hash_bytes(hash,buffer,got); total += got;
    } while (got != 0);
    if (total != size.LowPart) fail("length differs from EOF");
    if (!CloseHandle(h)) fail("CloseHandle");
    debugPrint("FILE READ OK\nbytes=%lu fnv=%08lX\n",(unsigned long)total,(unsigned long)hash);
    debugPrint("Streaming with open handle\n");
    h = open_image();
    for (;;) {
        if (!ReadFile(h,buffer,sizeof(buffer),&got,NULL)) fail("stream read");
        if (got == 0) {
            if (!CloseHandle(h)) fail("stream close");
            h = open_image();
        }
        // O handle permanece aberto durante a espera, ponto útil de checkpoint.
        Sleep(2000);
    }
}
