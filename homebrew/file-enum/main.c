// SPDX-License-Identifier: MIT
// Consumidor guest de teste; o runtime/kernel continuam Rust.
#include <windows.h>
#include <hal/video.h>
#include <hal/debug.h>
#include <xboxkrnl/xboxkrnl.h>
#include <stddef.h>
#include <string.h>

_Static_assert(offsetof(FILE_DIRECTORY_INFORMATION, FileName) == 64, "Xbox ANSI directory ABI");
static void fail(const char *reason) {
    debugPrint("FILE ENUM FAILED: %s\n",reason);
    for (;;) Sleep(2000);
}
static void end(HANDLE handle, WIN32_FIND_DATAA *data) {
    if (FindNextFileA(handle,data) || GetLastError()!=ERROR_NO_MORE_FILES) fail("end of enumeration");
}
int main(void) {
    XVideoSetMode(640,480,32,REFRESH_DEFAULT);
    WIN32_FIND_DATAA data;
    HANDLE dir=FindFirstFileA("D:\\*",&data);
    if (dir==INVALID_HANDLE_VALUE) fail("FindFirstFileA");
    if (strcmp(data.cFileName,"default.xbe") || data.nFileSizeHigh ||
        data.dwFileAttributes!=FILE_ATTRIBUTE_READONLY) fail("directory entry");
    DWORD listed=data.nFileSizeLow;
    HANDLE file=CreateFileA("D:\\default.xbe",GENERIC_READ,FILE_SHARE_READ,NULL,OPEN_EXISTING,FILE_ATTRIBUTE_NORMAL,NULL);
    if (file==INVALID_HANDLE_VALUE) fail("open enumerated file");
    LARGE_INTEGER size;
    if (!GetFileSizeEx(file,&size) || size.HighPart || size.LowPart!=listed) fail("enumerated length");
    if (!CloseHandle(file)) fail("close file");
    end(dir,&data);
    if (!FindClose(dir)) fail("FindClose");
    dir=FindFirstFileA("D:\\*.xbe",&data);
    if (dir==INVALID_HANDLE_VALUE || strcmp(data.cFileName,"default.xbe")) fail("filtered reopen");
    debugPrint("FILE ENUM OK\nname=default.xbe bytes=%lu\nFindNext: ERROR_NO_MORE_FILES\nOpen directory replay\n",(unsigned long)listed);
    for (;;) {
        Sleep(2000);
        end(dir,&data);
        // O handle permanece aberto. Reiniciar preserva o filtro inicial.
        unsigned char record[128];
        IO_STATUS_BLOCK io;
        if (NtQueryDirectoryFile(dir,NULL,NULL,NULL,&io,record,sizeof(record),FileDirectoryInformation,NULL,TRUE)!=STATUS_SUCCESS) fail("restart directory");
    }
}
