// SPDX-License-Identifier: MIT
// Sonda de referência: medir em Xbox/ambiente confiável, não calibrar pelo HLE.
#include <windows.h>
#include <hal/video.h>
#include <hal/debug.h>
#include <xboxkrnl/xboxkrnl.h>
#include <stdint.h>
#include <stddef.h>
#include <string.h>

_Static_assert(sizeof(OBJECT_DIRECTORY_INFORMATION)==12,"Xbox directory ABI");
_Static_assert(offsetof(OBJECT_DIRECTORY_INFORMATION,Type)==8,"Xbox Type offset");
typedef union {
    OBJECT_DIRECTORY_INFORMATION record;
    unsigned char bytes[256];
} RECORD;

static void query(HANDLE dir,ULONG length,BOOLEAN restart,ULONG *context) {
    RECORD buffer;
    ULONG returned=0xDEADBEEF;
    ULONG before=*context;
    memset(&buffer,0xA5,sizeof(buffer));
    NTSTATUS status=NtQueryDirectoryObject(dir,&buffer,length,restart,context,&returned);
    debugPrint("q len=%lu restart=%u ctx=%lu->%lu\n",length,(unsigned)restart,before,*context);
    debugPrint(" status=%08lX returned=%08lX\n",(ULONG)status,returned);
    if (status==STATUS_SUCCESS) {
        uintptr_t start=(uintptr_t)buffer.bytes;
        uintptr_t pointer=(uintptr_t)buffer.record.Name.Buffer;
        ULONG n=buffer.record.Name.Length;
        char name[49];
        debugPrint(" name_len=%u max=%u type=%08lX\n",buffer.record.Name.Length,
                   buffer.record.Name.MaximumLength,buffer.record.Type);
        if (pointer>=start && pointer-start<=sizeof(buffer.bytes) &&
            n<=sizeof(buffer.bytes)-(pointer-start)) {
            ULONG copied=n<48?n:48;
            memcpy(name,buffer.record.Name.Buffer,copied);name[copied]=0;
            debugPrint(" name=%s offset=%lu\n",name,(ULONG)(pointer-start));
        } else {
            debugPrint(" name buffer outside output: %08lX\n",(ULONG)pointer);
        }
    }
}
int main(void) {
    XVideoSetMode(640,480,32,REFRESH_DEFAULT);
    debugPrint("OBJECT DIRECTORY REFERENCE v1\n");
    HANDLE directory;
    OBJECT_ATTRIBUTES attributes;
    InitializeObjectAttributes(&attributes,NULL,OBJ_CASE_INSENSITIVE,ObDosDevicesDirectory(),NULL);
    NTSTATUS status=NtOpenDirectoryObject(&directory,&attributes);
    debugPrint("open dos devices: %08lX\n",(ULONG)status);
    if (status==STATUS_SUCCESS) {
        ULONG context=0;
        // Percurso limitado, sem presumir quantas unidades o console expõe.
        for (unsigned i=0;i<16;i++) {
            query(directory,256,i==0,&context);
            Sleep(1500);
        }
        context=0;
        for (unsigned i=0;i<4;i++) {
            const ULONG lengths[]={0,11,12,14};
            query(directory,lengths[i],TRUE,&context);
            Sleep(1500);
        }
        debugPrint("close: %08lX\n",(ULONG)NtClose(directory));
    }
    debugPrint("REFERENCE END (record video/log)\n");
    for (;;) Sleep(2000);
}
