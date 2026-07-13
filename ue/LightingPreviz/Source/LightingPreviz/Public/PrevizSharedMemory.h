// Copyright the unreal_lighting_previz authors.
//
// Read side of the previz shared-memory transport. This mirrors, byte-for-byte,
// the Rust writer in `previz/receiver/src/shmem.rs` (see that file / the
// receiver README for the authoritative layout). Single-writer (the receiver)
// / single-reader (this) seqlock, so frames are read tear-free.

#pragma once

#include "CoreMinimal.h"

// Header field layout (little-endian, native alignment). Kept as a POD so it can
// be overlaid directly on the mapped region.
#pragma pack(push, 1)
struct FPrevizVolumeHeader
{
    uint32 Magic;      // 0x315A5650 == "PVZ1"
    uint32 Version;    // 1
    uint32 NumVoxels;
    uint32 Width;
    uint32 Height;
    uint32 Length;
    uint32 CubeCount;
    uint32 Reserved;
    uint64 Seq;        // seqlock; even = stable, odd = writer in progress
};
#pragma pack(pop)

static_assert(sizeof(FPrevizVolumeHeader) == 40, "header must be 40 bytes");

namespace PrevizShm
{
    static constexpr uint32 Magic = 0x315A5650u; // "PVZ1"
    static constexpr uint32 Version = 1u;
    static constexpr int32  DataOffset = 64;     // matches Rust DATA_OFFSET
}

/**
 * Maps the receiver's POSIX shared-memory region read-only and pulls consistent
 * frame snapshots via the seqlock protocol. Not a UObject — owned by the
 * component below.
 */
class LIGHTINGPREVIZ_API FPrevizSharedMemory
{
public:
    FPrevizSharedMemory() = default;
    ~FPrevizSharedMemory();

    /** Open + mmap `/<ShmName>` read-only. Returns false (with a log) on failure
     *  or if the magic/version don't match. */
    bool Open(const FString& ShmName);
    void Close();
    bool IsOpen() const { return MappedPtr != nullptr; }

    /** Header view (valid while open). */
    const FPrevizVolumeHeader* Header() const
    {
        return IsOpen() ? reinterpret_cast<const FPrevizVolumeHeader*>(MappedPtr) : nullptr;
    }

    /** Copy the latest consistent frame into OutData (must be NumVoxels*3 bytes).
     *  Returns false if no stable snapshot was captured within MaxTries. */
    bool ReadFrame(uint8* OutData, int32 OutLen, int32 MaxTries = 16) const;

private:
    void* MappedPtr = nullptr;
    size_t MappedSize = 0;
};
