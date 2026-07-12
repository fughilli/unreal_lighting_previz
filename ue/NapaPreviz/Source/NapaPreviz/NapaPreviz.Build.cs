// Copyright Napa Lighted Art Festival previz.
using UnrealBuildTool;

public class NapaPreviz : ModuleRules
{
    public NapaPreviz(ReadOnlyTargetRules Target) : base(Target)
    {
        PCHUsage = PCHUsageMode.UseExplicitOrSharedPCHs;

        PublicDependencyModuleNames.AddRange(new string[]
        {
            "Core",
            "CoreUObject",
            "Engine",
        });

        // POSIX shm_open / mmap live in the platform's C library on Mac; no extra
        // link deps are needed. This module is Mac-only (see the .uplugin).
    }
}
