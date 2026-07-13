// Copyright the unreal_lighting_previz authors.
using UnrealBuildTool;

public class LightingPreviz : ModuleRules
{
    public LightingPreviz(ReadOnlyTargetRules Target) : base(Target)
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
