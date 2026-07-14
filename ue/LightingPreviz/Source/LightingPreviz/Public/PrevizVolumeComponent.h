// Copyright the unreal_lighting_previz authors.
#pragma once

#include "CoreMinimal.h"
#include "Components/ActorComponent.h"
#include "PrevizSharedMemory.h"
#include "RHITypes.h" // FUpdateTextureRegion2D
#include "PrevizVolumeComponent.generated.h"

class UInstancedStaticMeshComponent;
class UStaticMesh;
class UMaterialInterface;
class UMaterialInstanceDynamic;
class UTexture2D;

DECLARE_DYNAMIC_MULTICAST_DELEGATE(FPrevizFrameReceived);

/**
 * Drop this on an Actor to light a voxel volume from the previz receiver's
 * shared memory. On BeginPlay it connects to `/<ShmName>`, builds an
 * InstancedStaticMesh (one instance per voxel on a grid), and every frame
 * uploads the volume's RGB into a small transient texture (one bulk update —
 * per-instance data stays static, which is what keeps 56k voxels fast).
 *
 * To see color you need an emissive material with a texture parameter named
 * `VoxelTex`, sampled at the per-instance UV stored in Per Instance Custom
 * Data floats 0/1 (see previz/ue/README.md; setup_previz_level.py builds it).
 * Even without a material, the component logs frame/seq stats so you can
 * confirm the live feed is flowing.
 */
UCLASS(ClassGroup = (Previz), meta = (BlueprintSpawnableComponent))
class LIGHTINGPREVIZ_API UPrevizVolumeComponent : public UActorComponent
{
    GENERATED_BODY()

public:
    UPrevizVolumeComponent();

    /** POSIX shm object name the receiver publishes to (`--shm` flag). */
    UPROPERTY(EditAnywhere, BlueprintReadWrite, Category = "Previz")
    FString ShmName = TEXT("previz_volume");

    /** Try to (re)connect automatically until the receiver is up. */
    UPROPERTY(EditAnywhere, BlueprintReadWrite, Category = "Previz")
    bool bAutoConnect = true;

    /** Distance between voxel centers, in cm (15 cm pitch = 15.0 at 1:1 scale). */
    UPROPERTY(EditAnywhere, BlueprintReadWrite, Category = "Previz")
    float VoxelSpacing = 15.0f;

    /** Uniform scale applied to each voxel mesh. With the 100 cm engine cube,
     *  0.1 = 10 cm cubes → discrete glowing points at a 15 cm pitch. */
    UPROPERTY(EditAnywhere, BlueprintReadWrite, Category = "Previz")
    float VoxelScale = 0.1f;

    /** Mesh used for each voxel (a small cube or sphere). Required to render. */
    UPROPERTY(EditAnywhere, BlueprintReadWrite, Category = "Previz")
    TObjectPtr<UStaticMesh> VoxelMesh;

    /** Emissive material sampling texture parameter `VoxelTex` at the UV in
     *  Per Instance Custom Data 0/1. A dynamic instance of it gets the live
     *  volume texture bound at BeginPlay. */
    UPROPERTY(EditAnywhere, BlueprintReadWrite, Category = "Previz")
    TObjectPtr<UMaterialInterface> VoxelMaterial;

    /** Build one instance per voxel. Turn off to use OnFrameReceived only. */
    UPROPERTY(EditAnywhere, BlueprintReadWrite, Category = "Previz")
    bool bBuildInstances = true;

    // --- Vertical strands (the physical LED curtains) ------------------------

    /** Render a translucent/refractive rod per (x,y) column spanning the height. */
    UPROPERTY(EditAnywhere, BlueprintReadWrite, Category = "Previz|Strands")
    bool bBuildStrands = true;

    /** Mesh for each strand (a unit cube, stretched to a tall thin rod). */
    UPROPERTY(EditAnywhere, BlueprintReadWrite, Category = "Previz|Strands")
    TObjectPtr<UStaticMesh> StrandMesh;

    /** Refractive/translucent material for the strands. */
    UPROPERTY(EditAnywhere, BlueprintReadWrite, Category = "Previz|Strands")
    TObjectPtr<UMaterialInterface> StrandMaterial;

    /** Strand cross-section (cm). LEDs are ~3 mm; the diffuser rod is a bit wider. */
    UPROPERTY(EditAnywhere, BlueprintReadWrite, Category = "Previz|Strands")
    float StrandThickness = 1.0f;

    // --- Feed-driven lights (illuminate the surroundings) --------------------

    /** Spawn a coarse grid of point lights whose color/intensity track the
     *  volume, so the sculpture casts real colored light onto the environment.
     *  (Emissive alone doesn't light other surfaces.) */
    UPROPERTY(EditAnywhere, BlueprintReadWrite, Category = "Previz|Lights")
    bool bBuildLights = true;

    /** Number of light cells across the volume (X,Y,Z). Each cell is one light
     *  driven by the average of the lit voxels inside it. Default is a single
     *  light for the whole array — overlapping point-light volumes are the #1
     *  previz framerate killer, so raise this only with care. */
    UPROPERTY(EditAnywhere, BlueprintReadWrite, Category = "Previz|Lights")
    FIntVector LightGridResolution = FIntVector(1, 1, 1);

    /** Per-light brightness scale (candelas at full white). */
    UPROPERTY(EditAnywhere, BlueprintReadWrite, Category = "Previz|Lights")
    float LightIntensity = 100.0f;

    /** How far each light reaches, in cm. Also the hard cutoff beyond which the
     *  light contributes nothing — keep large so tall nearby buildings stay lit. */
    UPROPERTY(EditAnywhere, BlueprintReadWrite, Category = "Previz|Lights")
    float LightAttenuationRadius = 5000.0f;

    /** Soft source radius (cm) per light — turns the hard point into a glowing
     *  sphere so the layers blend into a smooth volume instead of banding. */
    UPROPERTY(EditAnywhere, BlueprintReadWrite, Category = "Previz|Lights")
    float LightSourceRadius = 150.0f;

    /** Max rate (Hz) at which feed data is pushed into instances/lights.
     *  The receiver publishes at ~70 fps; re-uploading every editor tick is
     *  wasted work. 0 = uncapped (push every tick). */
    UPROPERTY(EditAnywhere, BlueprintReadWrite, Category = "Previz")
    float MaxUpdateRateHz = 30.0f;

    /** Log a stats line this often (seconds). 0 disables. */
    UPROPERTY(EditAnywhere, BlueprintReadWrite, Category = "Previz")
    float LogEverySeconds = 1.0f;

    /** Fired after each successfully-read frame (for custom Blueprint logic). */
    UPROPERTY(BlueprintAssignable, Category = "Previz")
    FPrevizFrameReceived OnFrameReceived;

    /** Latest color for voxel (X,Y,Z). Returns black if out of range / no data. */
    UFUNCTION(BlueprintCallable, Category = "Previz")
    FColor GetVoxel(int32 X, int32 Y, int32 Z) const;

    /** True while the shared-memory region is mapped. */
    UFUNCTION(BlueprintCallable, Category = "Previz")
    bool IsConnected() const { return Shm.IsOpen(); }

    virtual void BeginPlay() override;
    virtual void EndPlay(const EEndPlayReason::Type Reason) override;
    virtual void TickComponent(float DeltaTime, ELevelTick TickType,
                               FActorComponentTickFunction* ThisTickFunction) override;

private:
    bool Connect();
    void BuildInstances();
    void BuildStrands();
    void BuildLights();
    void UpdateVoxelTexture();
    void UpdateLights();

    FPrevizSharedMemory Shm;

    // Cached geometry from the shm header.
    int32 NumVoxels = 0;
    int32 GridW = 0, GridH = 0, GridD = 0;

    // Latest frame, NumVoxels*3 bytes, (z,y,x) order.
    TArray<uint8> Frame;

    // Previous pushed frame, for skipping unchanged voxels.
    TArray<uint8> PrevFrame;

    float UpdateAccum = 0.0f;

    UPROPERTY(Transient)
    TObjectPtr<UInstancedStaticMeshComponent> Ism;

    /** Live volume colors: one texel per voxel, (x + y*W) across, z down. */
    UPROPERTY(Transient)
    TObjectPtr<UTexture2D> VoxelTex;

    UPROPERTY(Transient)
    TObjectPtr<UMaterialInstanceDynamic> VoxelMid;

    /** Double-buffered RGBA staging for UpdateTextureRegions (the previous
     *  upload may still be in flight on the render thread). */
    TArray<uint8> Staging[2];
    int32 StagingIndex = 0;
    FUpdateTextureRegion2D TexRegion;

    UPROPERTY(Transient)
    TObjectPtr<UInstancedStaticMeshComponent> StrandIsm;

    UPROPERTY(Transient)
    TArray<TObjectPtr<class UPointLightComponent>> Lights;

    double LastLogTime = 0.0;
    uint64 FrameCount = 0;
    float ReconnectAccum = 0.0f;
};
