// Copyright the unreal_lighting_previz authors.
#pragma once

#include "CoreMinimal.h"
#include "GameFramework/Actor.h"
#include "PrevizVolumeActor.generated.h"

class UPrevizVolumeComponent;

/**
 * A ready-to-place Actor that owns a UPrevizVolumeComponent (and a scene root
 * for it to build voxel instances under). Drag it into a level, or spawn it
 * from Python — `Volume` holds the component to configure.
 */
UCLASS(ClassGroup = (Previz))
class LIGHTINGPREVIZ_API APrevizVolumeActor : public AActor
{
    GENERATED_BODY()

public:
    APrevizVolumeActor();

    UPROPERTY(VisibleAnywhere, BlueprintReadOnly, Category = "Previz")
    TObjectPtr<UPrevizVolumeComponent> Volume;
};
