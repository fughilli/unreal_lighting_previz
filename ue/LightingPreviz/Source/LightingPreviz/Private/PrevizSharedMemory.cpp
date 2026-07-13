// Copyright the unreal_lighting_previz authors.
#include "PrevizSharedMemory.h"

#include <sys/mman.h>
#include <sys/stat.h>
#include <fcntl.h>
#include <unistd.h>

DEFINE_LOG_CATEGORY_STATIC(LogLightingPreviz, Log, All);

// Acquire-load of the 64-bit seqlock counter from the mapped region. Clang
// (which UE uses on Mac) provides the __atomic builtins.
static FORCEINLINE uint64 LoadSeqAcquire(const volatile uint64* Ptr)
{
    return __atomic_load_n(Ptr, __ATOMIC_ACQUIRE);
}

FPrevizSharedMemory::~FPrevizSharedMemory()
{
    Close();
}

bool FPrevizSharedMemory::Open(const FString& ShmName)
{
    Close();

    // POSIX shm names start with '/'. The receiver adds one too, so normalise.
    FString Name = ShmName;
    if (!Name.StartsWith(TEXT("/")))
    {
        Name = TEXT("/") + Name;
    }
    const FTCHARToUTF8 Utf8(*Name);

    const int Fd = shm_open(Utf8.Get(), O_RDONLY, 0);
    if (Fd < 0)
    {
        UE_LOG(LogLightingPreviz, Warning,
            TEXT("shm_open(%s) failed (errno %d) — is previz-receiver running?"),
            *Name, errno);
        return false;
    }

    struct stat St;
    if (fstat(Fd, &St) != 0 || St.st_size < (off_t)sizeof(FPrevizVolumeHeader))
    {
        UE_LOG(LogLightingPreviz, Warning, TEXT("shm %s too small / fstat failed"), *Name);
        close(Fd);
        return false;
    }

    void* Ptr = mmap(nullptr, (size_t)St.st_size, PROT_READ, MAP_SHARED, Fd, 0);
    close(Fd); // the mapping keeps the object alive
    if (Ptr == MAP_FAILED)
    {
        UE_LOG(LogLightingPreviz, Warning, TEXT("mmap(%s) failed (errno %d)"), *Name, errno);
        return false;
    }

    const FPrevizVolumeHeader* Hdr = reinterpret_cast<const FPrevizVolumeHeader*>(Ptr);
    if (Hdr->Magic != PrevizShm::Magic || Hdr->Version != PrevizShm::Version)
    {
        UE_LOG(LogLightingPreviz, Warning,
            TEXT("shm %s bad magic/version (0x%08x v%u)"), *Name, Hdr->Magic, Hdr->Version);
        munmap(Ptr, (size_t)St.st_size);
        return false;
    }

    MappedPtr = Ptr;
    MappedSize = (size_t)St.st_size;
    UE_LOG(LogLightingPreviz, Log,
        TEXT("Opened %s: %u voxels, %ux%ux%u, %u cube(s)"),
        *Name, Hdr->NumVoxels, Hdr->Width, Hdr->Height, Hdr->Length, Hdr->CubeCount);
    return true;
}

void FPrevizSharedMemory::Close()
{
    if (MappedPtr)
    {
        munmap(MappedPtr, MappedSize);
        MappedPtr = nullptr;
        MappedSize = 0;
    }
}

bool FPrevizSharedMemory::ReadFrame(uint8* OutData, int32 OutLen, int32 MaxTries) const
{
    if (!IsOpen())
    {
        return false;
    }
    const FPrevizVolumeHeader* Hdr = Header();
    const int32 Expected = (int32)Hdr->NumVoxels * 3;
    if (OutLen < Expected)
    {
        return false;
    }
    const uint8* Data = reinterpret_cast<const uint8*>(MappedPtr) + PrevizShm::DataOffset;
    const volatile uint64* SeqPtr = &reinterpret_cast<const volatile FPrevizVolumeHeader*>(MappedPtr)->Seq;

    for (int32 Try = 0; Try < MaxTries; ++Try)
    {
        const uint64 S1 = LoadSeqAcquire(SeqPtr);
        if (S1 & 1ull)
        {
            continue; // writer mid-update
        }
        FMemory::Memcpy(OutData, Data, Expected);
        const uint64 S2 = LoadSeqAcquire(SeqPtr);
        if (S1 == S2)
        {
            return true; // consistent snapshot
        }
    }
    return false;
}
