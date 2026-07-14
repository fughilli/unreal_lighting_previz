// Copyright the unreal_lighting_previz authors.
#include "PrevizVolumeComponent.h"

#include "Components/InstancedStaticMeshComponent.h"
#include "Components/PointLightComponent.h"
#include "Engine/StaticMesh.h"
#include "Engine/Texture2D.h"
#include "Materials/MaterialInstanceDynamic.h"

DEFINE_LOG_CATEGORY_STATIC(LogLightingPreviz, Log, All);

UPrevizVolumeComponent::UPrevizVolumeComponent()
{
    PrimaryComponentTick.bCanEverTick = true;
}

void UPrevizVolumeComponent::BeginPlay()
{
    Super::BeginPlay();
    Connect();
}

void UPrevizVolumeComponent::EndPlay(const EEndPlayReason::Type Reason)
{
    Shm.Close();
    Super::EndPlay(Reason);
}

bool UPrevizVolumeComponent::Connect()
{
    if (!Shm.Open(ShmName))
    {
        return false;
    }

    const FPrevizVolumeHeader* Hdr = Shm.Header();
    NumVoxels = (int32)Hdr->NumVoxels;
    GridW = FMath::Max(1, (int32)Hdr->Width);
    GridH = FMath::Max(1, (int32)Hdr->Height);
    // Depth from the actual buffer, so single-cube configs are always exact even
    // if world/length differ. (Multi-cube configs concatenate buffers — use a
    // single-cube config for the grid path; see README.)
    GridD = FMath::Max(1, NumVoxels / (GridW * GridH));

    Frame.SetNumZeroed(NumVoxels * 3);

    if (bBuildInstances)
    {
        BuildInstances();
    }
    if (bBuildStrands)
    {
        BuildStrands();
    }
    if (bBuildLights)
    {
        BuildLights();
    }
    return true;
}

void UPrevizVolumeComponent::BuildInstances()
{
    if (!VoxelMesh)
    {
        UE_LOG(LogLightingPreviz, Warning,
            TEXT("No VoxelMesh assigned — connected but rendering nothing. "
                 "Assign a cube mesh + emissive material to see voxels."));
        return;
    }

    // Live color texture: one texel per voxel, (x + y*W) across, z down. The
    // shm frame slice for layer z is already in (y,x) row order, so each
    // texture row is a straight copy of one z-slice.
    const int32 TexW = GridW * GridH;
    const int32 TexH = GridD;
    VoxelTex = UTexture2D::CreateTransient(TexW, TexH, PF_R8G8B8A8);
    VoxelTex->SRGB = false;
    VoxelTex->Filter = TF_Nearest;
    VoxelTex->NeverStream = true;
    VoxelTex->UpdateResource();
    TexRegion = FUpdateTextureRegion2D(0, 0, 0, 0, TexW, TexH);
    Staging[0].SetNumZeroed(TexW * TexH * 4);
    Staging[1].SetNumZeroed(TexW * TexH * 4);

    Ism = NewObject<UInstancedStaticMeshComponent>(GetOwner());
    Ism->SetupAttachment(GetOwner()->GetRootComponent());
    // Keep the per-frame-changing emissive instances out of Lumen's surface
    // cache and the distance field: with these on, every frame of feed data
    // invalidates the captured cards and Lumen re-captures the whole volume
    // continuously (tens of ms/frame). Light spill onto the environment comes
    // from the feed-driven light grid instead.
    Ism->bAffectDynamicIndirectLighting = false;
    Ism->bAffectDistanceFieldLighting = false;
    Ism->RegisterComponent();
    Ism->SetStaticMesh(VoxelMesh);
    if (VoxelMaterial)
    {
        VoxelMid = UMaterialInstanceDynamic::Create(VoxelMaterial, this);
        VoxelMid->SetTextureParameterValue(TEXT("VoxelTex"), VoxelTex);
        Ism->SetMaterial(0, VoxelMid);
    }
    // Two custom-data floats per instance: this voxel's UV into VoxelTex.
    // Set once here; per-frame color flows through the texture, so instance
    // data never changes again (GPUScene stays untouched at runtime).
    Ism->NumCustomDataFloats = 2;
    Ism->SetMobility(EComponentMobility::Movable);
    // Emissive points don't need shadows and there are a lot of them.
    Ism->SetCastShadow(false);
    // The LEDs are visual only — the camera/pawn flies straight through.
    Ism->SetCollisionEnabled(ECollisionEnabled::NoCollision);

    Ism->PreAllocateInstancesMemory(NumVoxels);
    for (int32 i = 0; i < NumVoxels; ++i)
    {
        const int32 z = i / (GridW * GridH);
        const int32 rem = i % (GridW * GridH);
        const int32 y = rem / GridW;
        const int32 x = rem % GridW;
        const FVector Loc(x * VoxelSpacing, y * VoxelSpacing, z * VoxelSpacing);
        Ism->AddInstance(FTransform(FQuat::Identity, Loc, FVector(VoxelScale)));
        const float Uv[2] = {
            (x + y * GridW + 0.5f) / TexW,
            (z + 0.5f) / TexH,
        };
        Ism->SetCustomData(i, MakeArrayView(Uv, 2), /*bMarkRenderStateDirty*/ false);
    }
    Ism->MarkRenderStateDirty();

    UE_LOG(LogLightingPreviz, Log, TEXT("Built %d voxel instances (%dx%dx%d, tex %dx%d)"),
        NumVoxels, GridW, GridH, GridD, TexW, TexH);
}

void UPrevizVolumeComponent::BuildStrands()
{
    if (!StrandMesh)
    {
        return; // strands are optional; no mesh -> just the LED points
    }

    StrandIsm = NewObject<UInstancedStaticMeshComponent>(GetOwner());
    StrandIsm->SetupAttachment(GetOwner()->GetRootComponent());
    StrandIsm->bAffectDynamicIndirectLighting = false;
    StrandIsm->bAffectDistanceFieldLighting = false;
    StrandIsm->RegisterComponent();
    StrandIsm->SetStaticMesh(StrandMesh);
    if (StrandMaterial)
    {
        StrandIsm->SetMaterial(0, StrandMaterial);
    }
    StrandIsm->SetCastShadow(false);
    StrandIsm->SetMobility(EComponentMobility::Movable);
    StrandIsm->SetCollisionEnabled(ECollisionEnabled::NoCollision);
    // Put strands on lighting Channel 1 so the feed-driven lights (default
    // Channel 0, which light the terrain) don't also illuminate the sculpture.
    StrandIsm->SetLightingChannels(false, true, false);

    // One rod per (x,y) column, spanning the full vertical (z) extent. The engine
    // cube is 100 cm, so scale X/Y to the strand cross-section and Z to the height.
    const float HeightCm = (GridD - 1) * VoxelSpacing + VoxelSpacing; // +1 pitch of margin
    const float MidZ = (GridD - 1) * VoxelSpacing * 0.5f;
    const FVector Scale(StrandThickness / 100.0f, StrandThickness / 100.0f, HeightCm / 100.0f);

    StrandIsm->PreAllocateInstancesMemory(GridW * GridH);
    for (int32 y = 0; y < GridH; ++y)
    {
        for (int32 x = 0; x < GridW; ++x)
        {
            const FVector Loc(x * VoxelSpacing, y * VoxelSpacing, MidZ);
            StrandIsm->AddInstance(FTransform(FQuat::Identity, Loc, Scale));
        }
    }

    UE_LOG(LogLightingPreviz, Log, TEXT("Built %d strands (thickness %.1f cm, height %.0f cm)"),
        GridW * GridH, StrandThickness, HeightCm);
}

void UPrevizVolumeComponent::TickComponent(float DeltaTime, ELevelTick TickType,
    FActorComponentTickFunction* ThisTickFunction)
{
    Super::TickComponent(DeltaTime, TickType, ThisTickFunction);

    if (!Shm.IsOpen())
    {
        if (bAutoConnect)
        {
            ReconnectAccum += DeltaTime;
            if (ReconnectAccum >= 0.5f)
            {
                ReconnectAccum = 0.0f;
                Connect();
            }
        }
        return;
    }

    // Cap how often we push data into the scene: the receiver publishes at
    // ~70 fps, but re-uploading 8000 instances' custom data at editor tick
    // rate costs more than the visual difference is worth.
    if (MaxUpdateRateHz > 0.0f)
    {
        UpdateAccum += DeltaTime;
        const float Interval = 1.0f / MaxUpdateRateHz;
        if (UpdateAccum < Interval)
        {
            return;
        }
        UpdateAccum = FMath::Min(UpdateAccum - Interval, Interval);
    }

    if (!Shm.ReadFrame(Frame.GetData(), Frame.Num()))
    {
        return; // couldn't grab a tear-free snapshot this tick; try next
    }
    ++FrameCount;

    if (bBuildInstances && Ism)
    {
        UpdateVoxelTexture();
    }
    if (bBuildLights && Lights.Num() > 0)
    {
        UpdateLights();
    }

    OnFrameReceived.Broadcast();

    if (LogEverySeconds > 0.0f)
    {
        const double Now = FPlatformTime::Seconds();
        if (Now - LastLogTime >= LogEverySeconds)
        {
            LastLogTime = Now;
            const FColor C = GetVoxel(GridW / 2, GridH / 2, GridD / 2);
            UE_LOG(LogLightingPreviz, Log,
                TEXT("frames=%llu  center voxel=(%d,%d,%d)"),
                FrameCount, C.R, C.G, C.B);
        }
    }
}

void UPrevizVolumeComponent::UpdateVoxelTexture()
{
    // One bulk texture upload per frame (~NumVoxels*4 bytes) instead of
    // per-instance custom-data churn — instances stay static on the GPU.
    if (!VoxelTex || Staging[0].Num() != NumVoxels * 4)
    {
        return;
    }
    // Skip entirely when the feed didn't change since the last push.
    if (PrevFrame.Num() == Frame.Num()
        && FMemory::Memcmp(PrevFrame.GetData(), Frame.GetData(), Frame.Num()) == 0)
    {
        return;
    }

    // Fill the idle staging buffer; the other one may still be in flight on
    // the render thread from the previous UpdateTextureRegions call.
    uint8* Dst = Staging[StagingIndex].GetData();
    StagingIndex ^= 1;
    const uint8* Src = Frame.GetData();
    for (int32 i = 0; i < NumVoxels; ++i)
    {
        Dst[i * 4 + 0] = Src[i * 3 + 0];
        Dst[i * 4 + 1] = Src[i * 3 + 1];
        Dst[i * 4 + 2] = Src[i * 3 + 2];
        Dst[i * 4 + 3] = 255;
    }
    VoxelTex->UpdateTextureRegions(
        /*MipIndex*/ 0, /*NumRegions*/ 1, &TexRegion,
        /*SrcPitch*/ (uint32)(TexRegion.Width * 4), /*SrcBpp*/ 4, Dst);

    PrevFrame = Frame;
}

void UPrevizVolumeComponent::BuildLights()
{
    const int32 GX = FMath::Max(1, LightGridResolution.X);
    const int32 GY = FMath::Max(1, LightGridResolution.Y);
    const int32 GZ = FMath::Max(1, LightGridResolution.Z);

    USceneComponent* Root = GetOwner()->GetRootComponent();
    Lights.Reset();
    Lights.Reserve(GX * GY * GZ);

    for (int32 cz = 0; cz < GZ; ++cz)
    {
        for (int32 cy = 0; cy < GY; ++cy)
        {
            for (int32 cx = 0; cx < GX; ++cx)
            {
                // Cell center in voxel coordinates -> Unreal-space location.
                const float vx = (cx + 0.5f) * GridW / GX;
                const float vy = (cy + 0.5f) * GridH / GY;
                const float vz = (cz + 0.5f) * GridD / GZ;
                const FVector Loc(vx * VoxelSpacing, vy * VoxelSpacing, vz * VoxelSpacing);

                UPointLightComponent* Light =
                    NewObject<UPointLightComponent>(GetOwner());
                Light->SetupAttachment(Root);
                Light->SetMobility(EComponentMobility::Movable);
                Light->RegisterComponent();
                Light->SetRelativeLocation(Loc);
                Light->SetAttenuationRadius(LightAttenuationRadius);
                Light->SetSourceRadius(LightSourceRadius); // soft sphere -> smooth blend
                Light->SetCastShadows(false); // many lights; shadows too costly
                Light->SetIntensityUnits(ELightUnits::Candelas);
                Light->SetIntensity(0.0f);
                Lights.Add(Light);
            }
        }
    }

    UE_LOG(LogLightingPreviz, Log, TEXT("Built %d feed-driven lights (%dx%dx%d)"),
        Lights.Num(), GX, GY, GZ);
}

void UPrevizVolumeComponent::UpdateLights()
{
    const int32 GX = FMath::Max(1, LightGridResolution.X);
    const int32 GY = FMath::Max(1, LightGridResolution.Y);
    const int32 GZ = FMath::Max(1, LightGridResolution.Z);
    const int32 NumCells = GX * GY * GZ;
    if (Lights.Num() != NumCells)
    {
        return;
    }

    // Accumulate average RGB per cell in one pass over the volume.
    TArray<FVector> Sum;
    Sum.SetNumZeroed(NumCells);
    TArray<int32> Count;
    Count.SetNumZeroed(NumCells);

    for (int32 z = 0; z < GridD; ++z)
    {
        const int32 cz = FMath::Min(z * GZ / GridD, GZ - 1);
        for (int32 y = 0; y < GridH; ++y)
        {
            const int32 cy = FMath::Min(y * GY / GridH, GY - 1);
            for (int32 x = 0; x < GridW; ++x)
            {
                const int32 cx = FMath::Min(x * GX / GridW, GX - 1);
                const int32 cell = cx + cy * GX + cz * GX * GY;
                const int32 i = (z * GridW * GridH + y * GridW + x) * 3;
                Sum[cell] += FVector(Frame[i], Frame[i + 1], Frame[i + 2]);
                ++Count[cell];
            }
        }
    }

    const float Inv = 1.0f / 255.0f;
    for (int32 cell = 0; cell < NumCells; ++cell)
    {
        UPointLightComponent* Light = Lights[cell];
        if (!Light || Count[cell] == 0)
        {
            continue;
        }
        const FVector Avg = (Sum[cell] / Count[cell]) * Inv; // 0..1 RGB
        const float Brightness = FMath::Max3(Avg.X, Avg.Y, Avg.Z);
        if (Brightness <= KINDA_SMALL_NUMBER)
        {
            if (Light->Intensity != 0.0f)
            {
                Light->SetIntensity(0.0f);
            }
            continue;
        }
        // Hue from the normalized color; intensity scaled by brightness.
        // Skip the setters when nothing changed — they push render commands.
        const FLinearColor Color(Avg.X / Brightness, Avg.Y / Brightness, Avg.Z / Brightness);
        const float NewIntensity = LightIntensity * Brightness;
        if (Light->LightColor != Color.ToFColor(/*bSRGB*/ true))
        {
            Light->SetLightColor(Color);
        }
        if (!FMath::IsNearlyEqual(Light->Intensity, NewIntensity, 0.01f))
        {
            Light->SetIntensity(NewIntensity);
        }
    }
}

FColor UPrevizVolumeComponent::GetVoxel(int32 X, int32 Y, int32 Z) const
{
    if (X < 0 || X >= GridW || Y < 0 || Y >= GridH || Z < 0 || Z >= GridD)
    {
        return FColor::Black;
    }
    const int32 i = Z * (GridW * GridH) + Y * GridW + X;
    if ((i + 1) * 3 > Frame.Num())
    {
        return FColor::Black;
    }
    return FColor(Frame[i * 3 + 0], Frame[i * 3 + 1], Frame[i * 3 + 2]);
}
